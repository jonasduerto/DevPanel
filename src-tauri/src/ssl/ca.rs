use std::fs;
use std::path::{Path, PathBuf};

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
};
use time::{Duration, OffsetDateTime};

/// CommonName of the local root CA. Also used as the match string when
/// purging a stale copy from the Windows Root store during (re)trust.
const CA_COMMON_NAME: &str = "DevPanel Local CA";

pub struct CertPaths {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
}

/// A local root CA (mkcert-style): one self-signed key pair, generated
/// once and reused to sign a leaf certificate per workspace domain.
/// Persisted as plain PEM files under `{root}/data/ca/` — nothing here
/// touches the OS trust store until `trust()` is explicitly called.
pub struct CertificateAuthority {
    root: PathBuf,
    key_pem: String,
    cert_pem: String,
}

impl CertificateAuthority {
    fn ca_dir(root: &Path) -> PathBuf {
        root.join("data").join("ca")
    }

    fn key_path(root: &Path) -> PathBuf {
        Self::ca_dir(root).join("devpanel-ca.key")
    }

    fn cert_path(root: &Path) -> PathBuf {
        Self::ca_dir(root).join("devpanel-ca.crt")
    }

    pub fn load_or_create(root: &Path) -> Result<Self, String> {
        let dir = Self::ca_dir(root);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let key_path = Self::key_path(root);
        let cert_path = Self::cert_path(root);

        if key_path.exists() && cert_path.exists() {
            let key_pem = fs::read_to_string(&key_path).map_err(|e| e.to_string())?;
            let cert_pem = fs::read_to_string(&cert_path).map_err(|e| e.to_string())?;
            return Ok(Self {
                root: root.to_path_buf(),
                key_pem,
                cert_pem,
            });
        }

        let mut params = CertificateParams::new(Vec::<String>::new()).map_err(|e| e.to_string())?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        // rcgen's own defaults (1975-4096) are rejected outright by Chrome
        // as malformed rather than just "untrusted" — a validity period
        // that extreme trips certificate-parsing sanity checks. A locally
        // trusted root isn't subject to the public CA/Browser Forum's
        // leaf-cert lifetime caps, so a long-but-sane validity is fine here.
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(1);
        params.not_after = now + Duration::days(3650);
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, CA_COMMON_NAME);
        dn.push(DnType::OrganizationName, "DevPanel");
        params.distinguished_name = dn;

        let key_pair = KeyPair::generate().map_err(|e| e.to_string())?;
        let cert = params.self_signed(&key_pair).map_err(|e| e.to_string())?;
        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();

        fs::write(&key_path, &key_pem).map_err(|e| e.to_string())?;
        fs::write(&cert_path, &cert_pem).map_err(|e| e.to_string())?;
        // This is a brand-new CA: whatever the trust store held before (a
        // stale CA from a prior install) no longer matches the certs we now
        // issue, so drop any lingering "trusted" marker. is_trusted() then
        // reports false and the UI re-prompts, and the next trust() purges the
        // old store entry before installing this one.
        let _ = fs::remove_file(dir.join(".trusted"));

        Ok(Self {
            root: root.to_path_buf(),
            key_pem,
            cert_pem,
        })
    }

    pub fn is_trusted(&self) -> bool {
        Self::ca_dir(&self.root).join(".trusted").exists()
    }

    /// Installs the CA cert into the Windows Root trust store. Only ever
    /// called from the explicit "Trust this CA" button in Settings — never
    /// automatically during workspace/domain setup. Requires elevation.
    pub fn trust(&self) -> Result<(), String> {
        super::elevate::install_ca_elevated(&Self::cert_path(&self.root), CA_COMMON_NAME)?;
        fs::write(Self::ca_dir(&self.root).join(".trusted"), "1").map_err(|e| e.to_string())
    }

    /// Issues a leaf certificate for `domain`, signed by this CA. Purely
    /// local file writes — no elevation, no system state touched.
    pub fn issue_cert(&self, domain: &str, out_dir: &Path) -> Result<CertPaths, String> {
        fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;

        let mut leaf_params =
            CertificateParams::new(vec![domain.to_string()]).map_err(|e| e.to_string())?;
        // Same reasoning as the CA cert: rcgen's 1975-4096 defaults get
        // rejected outright by Chrome as malformed. 398 days keeps this
        // under every browser's leaf-certificate lifetime cap, matching
        // mkcert's own convention for locally-trusted dev certs.
        let now = OffsetDateTime::now_utc();
        leaf_params.not_before = now - Duration::days(1);
        leaf_params.not_after = now + Duration::days(398);
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, domain);
        leaf_params.distinguished_name = dn;

        let leaf_key = KeyPair::generate().map_err(|e| e.to_string())?;
        let ca_key = KeyPair::from_pem(&self.key_pem).map_err(|e| e.to_string())?;
        let issuer = Issuer::from_ca_cert_pem(&self.cert_pem, ca_key).map_err(|e| e.to_string())?;
        let leaf_cert = leaf_params
            .signed_by(&leaf_key, &issuer)
            .map_err(|e| e.to_string())?;

        let cert_file = out_dir.join(format!("{domain}.crt"));
        let key_file = out_dir.join(format!("{domain}.key"));
        fs::write(&cert_file, leaf_cert.pem()).map_err(|e| e.to_string())?;
        fs::write(&key_file, leaf_key.serialize_pem()).map_err(|e| e.to_string())?;

        Ok(CertPaths {
            cert_file,
            key_file,
        })
    }
}

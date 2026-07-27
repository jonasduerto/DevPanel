
<VirtualHost *:443>
    ServerName {domain}
    DocumentRoot "{doc_root}"
    SSLEngine on
    SSLCertificateFile "{cert_file}"
    SSLCertificateKeyFile "{key_file}"
    <Directory "{doc_root}">
        AllowOverride All
        Require all granted
        DirectoryIndex index.php index.html
    </Directory>
</VirtualHost>

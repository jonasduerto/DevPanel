<VirtualHost *:{listen_port}>
    ServerName {domain}
    DocumentRoot "{doc_root}"
    <Directory "{doc_root}">
        AllowOverride All
        Require all granted
        DirectoryIndex index.php index.html
    </Directory>
</VirtualHost>

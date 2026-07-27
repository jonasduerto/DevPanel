server {
    listen {listen_port};
{ssl_block}    server_name {domain};
    root "{doc_root}";
    index index.php index.html;

    location / {
        try_files $uri $uri/ /index.php?$query_string;
    }

    location ~ \.php$ {
        include fastcgi_params;
        fastcgi_pass 127.0.0.1:{php_port};
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
    }
}

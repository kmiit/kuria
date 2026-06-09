# Nginx reverse proxy

Kuria serves the Web UI and API over HTTP. In production, the recommended setup is to let Nginx terminate HTTPS and proxy requests to Kuria.

## Kuria config

Use `tls.mode = "external"` when certificates are handled outside Kuria:

```toml
[web]
listen_addr = "127.0.0.1:8080"
trust_proxy_headers = true

[tls]
mode = "external"
cert_path = "./data/certs/cert.pem"
key_path = "./data/certs/key.pem"
```

With `external`, Kuria will not load certificates and will not enable Kuria-managed SMTPS, IMAPS, or STARTTLS. If you want Kuria itself to serve mail TLS, use `mode = "internal"` and provide `cert_path` and `key_path`.

## Nginx config

```nginx
server {
    listen 80;
    server_name mail.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    server_name mail.example.com;

    ssl_certificate /etc/letsencrypt/live/mail.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/mail.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;

        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

Only enable `trust_proxy_headers` when Kuria listens behind a trusted reverse proxy. If Kuria is reachable directly by clients, leave it disabled so clients cannot spoof forwarding headers.

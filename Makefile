CARGO ?= cargo

.PHONY: test check release dev-cert

test:
	$(CARGO) test --all-targets

check:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --all-targets -- -D warnings

release:
	$(CARGO) build --release --locked

dev-cert:
	mkdir -p var
	openssl req -x509 -newkey rsa:2048 -sha256 -days 30 -nodes \
		-keyout var/dev-key.pem -out var/dev-cert.pem \
		-subj "/CN=localhost" \
		-addext "subjectAltName=DNS:localhost,IP:127.0.0.1"
	chmod 600 var/dev-key.pem

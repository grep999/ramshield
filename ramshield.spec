Name:        ramshield
Version:     0.1.0
Release:     1%{?dist}
Summary:     RamShield Autonomous Ingress Defense
License:     MIT
URL:         https://github.com/grep999/ramshield
Source0:     %{name}-%{version}.tar.gz
BuildRequires: python3, systemd
Requires: python3, curl, systemd

%description
RamShield is a DDoS mitigation reverse proxy and enforcement layer.
It operates at the network edge to detect and block malicious traffic.

%prep
%setup -q

%build
# No build needed for pre-compiled binary

%install
install -m 0755 -d %{buildroot}/usr/local/bin
install -m 0755 -d %{buildroot}/etc/ramshield
install -m 0755 ramshield %{buildroot}/usr/local/bin/ramshield
install -m 0640 config-min.toml %{buildroot}/etc/ramshield/config.toml

%files
/usr/local/bin/ramshield
/etc/ramshield/config.toml

%post
systemctl daemon-reload || true
systemctl enable ramshield.service || true

%preun
systemctl stop ramshield.service || true
systemctl disable ramshield.service || true

%changelog
* $(date '+%a %b %d %Y') RamShield <contact@ramshield.io> - 0.1.0-1
- Initial RPM package for RamShield

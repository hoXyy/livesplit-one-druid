%global srcname livesplit-one-druid
%global _disable_source_fetch 0

%if 0%{?commit:1}
%global srcver %{commit}
%else
%global srcver %(git rev-parse HEAD 2>/dev/null || echo HEAD)
%endif

%if ! 0%{?commit_short:1}
%global commit_short %(c=%{srcver}; if [ "$c" = HEAD ]; then git rev-parse --short HEAD 2>/dev/null || echo nogit; else printf '%s' "$c" | cut -c1-7; fi)
%endif

%if ! 0%{?date:1}
%global date %(date +%Y%m%d)
%endif

%global rel 1.%{date}.git%{commit_short}%{?dist}

Name: livesplit-one-druid
Version: 0.7.2
Release: %{rel}
Summary: A desktop version of LiveSplit One (hoxi's fork).

License: MIT
URL: https://github.com/hoXyy/livesplit-one-druid
Source0: https://github.com/hoXyy/%{srcname}/archive/%{srcver}/%{srcname}-%{srcver}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  git
BuildRequires:  gtk3-devel
BuildRequires:  pkgconfig
BuildRequires:  desktop-file-utils
BuildRequires:  glib2-devel
BuildRequires:  cairo-devel
BuildRequires:  pango-devel
BuildRequires:  atk-devel
BuildRequires:  gdk-pixbuf2-devel

%description
A desktop version of LiveSplit One (hoxi's fork).

%prep
%setup -T -c -n %{srcname}-%{version}
zcat %{SOURCE0} | tar xf - --strip-components=1

%build
cargo build --release %{?_smp_mflags}

%install
install -Dpm 0755 target/release/livesplit-one %{buildroot}%{_bindir}/%{name}
install -Dpm 0644 packaging/%{name}.desktop %{buildroot}%{_datadir}/applications/%{name}.desktop
install -Dpm 0644 icons/icon.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/%{name}.svg
install -Dpm 0644 icons/icon.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/%{name}.png
desktop-file-validate %{buildroot}%{_datadir}/applications/%{name}.desktop

%files
%{_bindir}/%{name}
%{_datadir}/applications/%{name}.desktop
%{_datadir}/icons/hicolor/scalable/apps/%{name}.svg
%{_datadir}/icons/hicolor/256x256/apps/%{name}.png
%doc README.md

%changelog
* Sat Jun 13 2026 Automatic COPR Build <noreply@copr.fedorainfracloud.org> 0.7.2-1
- Initial package

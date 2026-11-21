# Built from a prepared tree by packaging/rpm/build-in-container.sh:
# binaries are compiled before rpmbuild, which only stages and packages them.
Name:           3dmouse-linux
Version:        %{sm_version}
Release:        1%{?dist}
Summary:        Userspace driver and settings for 3Dconnexion 3D mice
License:        GPL-3.0-or-later
URL:            https://github.com/JoshShearer/3dmouse-linux
BuildRequires:  systemd-rpm-macros
Requires:       systemd, systemd-udev
Requires:       qt6-qtdeclarative, qt6-qtwayland, layer-shell-qt
Recommends:     libspnav, libnotify
# the pre-release name of this package
Obsoletes:      spacemouse-linux < %{version}-%{release}
Suggests:       spacenavd
%global debug_package %{nil}
%global __brp_strip_static_archive %{nil}

%description
3dmoused owns the device through hidraw/evdev, serves the libspnav
protocol so Blender, FreeCAD, KiCad and OpenSCAD work natively, and injects
desktop shortcuts for button actions. 3dmouse-agent switches profiles per
focused application; 3dmouse-config is the Qt settings app. Coexists with
spacenavd: if spacenavd is running it keeps the device.

%install
%{sm_tree}/packaging/install-tree.sh %{buildroot} gnome45
gzip -9n %{buildroot}%{_mandir}/man*/*

%post
%systemd_post 3dmoused.service
%systemd_user_post 3dmouse-agent.service 3dmouse-osd.service
udevadm control --reload >/dev/null 2>&1 || :
udevadm trigger --action=change --subsystem-match=hidraw --subsystem-match=input >/dev/null 2>&1 || :
modprobe uinput >/dev/null 2>&1 || :
if [ $1 -eq 1 ] && [ -d /run/systemd/system ]; then
    systemctl enable --now 3dmoused.service >/dev/null 2>&1 || :
    systemctl --global enable 3dmouse-agent.service 3dmouse-osd.service >/dev/null 2>&1 || :
fi
if [ $1 -gt 1 ]; then
    echo "3DMouse Linux upgraded: reopen running 3D apps (libspnav does not reconnect)."
else
    echo "3DMouse Linux: log out and back in (or run 'systemctl --user start 3dmouse-agent')."
fi

%preun
%systemd_preun 3dmoused.service
%systemd_user_preun 3dmouse-agent.service 3dmouse-osd.service

%postun
%systemd_postun_with_restart 3dmoused.service
udevadm control --reload >/dev/null 2>&1 || :

%files
%license /usr/share/doc/3dmouse-linux/LICENSE
/usr/bin/3dmoused
/usr/bin/3dmouse-agent
/usr/bin/3dmousectl
/usr/bin/3dmouse-config
/usr/bin/3dmouse-osd
/usr/bin/3dmouse-tray
%{_unitdir}/3dmoused.service
%{_userunitdir}/3dmouse-agent.service
%{_userunitdir}/3dmouse-osd.service
%{_userunitdir}/3dmouse-tray.service
%{_udevrulesdir}/70-3dmouse-linux.rules
%{_modulesloaddir}/3dmouse-linux.conf
%{_datadir}/applications/io.github.joshshearer._3dmouse_linux.config.desktop
%{_datadir}/icons/hicolor/scalable/apps/3dmouse-linux.svg
%{_datadir}/gnome-shell/extensions/3dmouse-linux@joshshearer.github.io/
%dir /usr/share/doc/3dmouse-linux
%doc /usr/share/doc/3dmouse-linux/*.md
%{_mandir}/man1/*
%{_mandir}/man8/*

%changelog
* Tue Nov 10 2026 3DMouse Linux contributors <noreply@invalid> - 0.1.0-1
- First alpha

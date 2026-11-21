import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: page
    property bool active: false
    property var s: null
    clip: true
    function load() { agent.call("settings.get", {}, function(ok, r) { if (ok) page.s = r }) }
    function set(k, v) { var o = {}; o[k] = v; agent.call("settings.set", {"settings": o}, function(ok, r) { if (ok) page.s = r; else win.toast(r) }) }
    // the tray is a systemd user service, not an agent setting: it must survive the agent restarting
    property bool trayOn: false
    property bool trayBusy: false
    function loadTray() { agent.userUnit("is-enabled", "3dmouse-tray.service", function(ok, out) { page.trayOn = ok }) }
    function setTray(on) {
        page.trayBusy = true
        agent.userUnit(on ? "enable" : "disable", "3dmouse-tray.service", function(ok, out) {
            page.trayBusy = false
            if (!ok) win.toast(qsTr("Could not change the status icon: ") + out)
            page.loadTray()
        })
    }
    onActiveChanged: if (active) { load(); loadTray() }
    ColumnLayout {
        width: page.availableWidth
        spacing: 12
        Item { height: 4 }
        Card {
            title: qsTr("Behaviour")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Switch { text: qsTr("Switch profiles automatically with the focused application"); checked: page.s ? page.s.auto_switch : true; onToggled: page.set("auto_switch", checked) }
            Switch { text: qsTr("Show notifications (profile changes, toggles)"); checked: page.s ? page.s.osd_enabled : true; onToggled: page.set("osd_enabled", checked) }
            Switch {
                text: qsTr("Show a status icon in the panel (tray)")
                checked: page.trayOn
                enabled: !page.trayBusy
                onToggled: page.setTray(checked)
            }
            Label {
                visible: page.trayOn
                text: qsTr("The icon shows the device and active profile and switches profiles. It needs a panel with a tray (waybar, KDE, COSMIC, XFCE; on GNOME, the AppIndicator extension).")
                wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.75
            }
            Switch {
                text: qsTr("Allow the virtual joystick (game controller profiles)")
                checked: page.s ? !!page.s.allow_virtual_joystick : false
                onToggled: page.set("allow_virtual_joystick", checked)
            }
            Button { text: qsTr("Run setup…"); onClicked: win.openSetup() }
            RowLayout {
                Label { text: qsTr("Overlay position") }
                ComboBox {
                    readonly property var vals: ["top", "center", "bottom"]
                    model: [qsTr("Top"), qsTr("Centre"), qsTr("Bottom")]
                    currentIndex: page.s && vals.indexOf(page.s.osd_position) >= 0 ? vals.indexOf(page.s.osd_position) : 2
                    Accessible.name: qsTr("Overlay position")
                    onActivated: page.set("osd_position", vals[currentIndex])
                }
                Label { text: qsTr("Size") }
                SpinBox {
                    from: 75; to: 200; stepSize: 25
                    value: page.s && page.s.osd_scale ? Math.round(page.s.osd_scale * 100) : 100
                    textFromValue: function(v) { return v + " %" }
                    valueFromText: function(t) { return parseInt(t) }
                    Accessible.name: qsTr("Overlay size")
                    onValueModified: page.set("osd_scale", value / 100)
                }
            }
            Label {
                text: agent.status && agent.status.agent && agent.status.agent.capabilities && agent.status.agent.capabilities.osd_overlay
                      ? qsTr("On-screen overlay connected.") : qsTr("No overlay connected: messages appear as desktop notifications (always the case on GNOME Wayland).")
                wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.75
            }
            Label { text: page.s && page.s.manual_profile ? qsTr("Pinned profile: %1").arg(page.s.manual_profile) : qsTr("No pinned profile"); }
        }
        Card {
            title: qsTr("Security")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Switch { text: qsTr("Allow command actions"); checked: page.s ? page.s.allow_exec : false; onToggled: page.set("allow_exec", checked) }
            Label {
                text: qsTr("When enabled, buttons can run programs you configured yourself. Commands never run through a shell, never run while the screen is locked, and commands in imported profiles stay disabled until you confirm them.")
                wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.75
            }
        }
        Card {
            title: qsTr("About")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Label { text: qsTr("3DMouse Linux %1 – GPL-3.0-or-later").arg(agent.version) }
            Label { text: qsTr("Agent socket: %1").arg(agent.socketPath); opacity: 0.7 }
            Label { text: agent.status && agent.status.agent ? qsTr("Profiles: %1").arg(agent.status.agent.config_dir) : ""; opacity: 0.7 }
            Label { text: qsTr("Not affiliated with 3Dconnexion. SpaceMouse is a trademark of 3Dconnexion."); opacity: 0.7; wrapMode: Text.Wrap; Layout.fillWidth: true }
        }
        Item { Layout.fillHeight: true }
    }
}

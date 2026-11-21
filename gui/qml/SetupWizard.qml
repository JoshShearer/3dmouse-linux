import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// First-run wizard (SRS FR-700): identify device, validate input, detect the
// desktop, check native app integration, pick a sensible default, verify a
// button, save. Never asks for sudo; it only reads status and edits the
// user's own profiles and settings through the agent.
Dialog {
    id: wiz
    objectName: "setupWizard"
    modal: true
    title: qsTr("Set up your 3D mouse")
    width: Math.min(parent ? parent.width - 48 : 640, 680)
    height: Math.min(parent ? parent.height - 48 : 560, 580)
    anchors.centerIn: parent
    closePolicy: Popup.CloseOnEscape

    property int step: 0
    readonly property int lastStep: 5
    readonly property var st: agent.status
    readonly property var b: st && st.broker ? st.broker : null
    readonly property var dev: b && b.devices && b.devices.length ? b.devices[0] : null
    readonly property var ag: st && st.agent ? st.agent : null
    // axis test: highest |normalised| value seen per axis
    property var peak: [0, 0, 0, 0, 0, 0]
    property var seenButtons: ({})
    property string unmatched: "native"   // "native" or "scroll"
    readonly property var axisNames: [qsTr("Pan right/left"), qsTr("Lift/press"), qsTr("Push/pull"), qsTr("Tilt forward/back"), qsTr("Twist"), qsTr("Tilt left/right")]
    readonly property int axesDone: peak.filter(function(v) { return v >= 0.5 }).length
    readonly property int buttonsDone: Object.keys(seenButtons).length

    onOpened: { step = 0; peak = [0, 0, 0, 0, 0, 0]; seenButtons = {} }
    onStepChanged: agent.live = visible && (step === 2)
    onClosed: agent.live = false

    Connections {
        target: agent
        enabled: wiz.visible && wiz.step === 2
        function onMotion() {
            var n = agent.norm, p = wiz.peak.slice(), changed = false
            for (var i = 0; i < 6 && i < n.length; i++) {
                var a = Math.abs(n[i])
                if (a > p[i]) { p[i] = a; changed = true }
            }
            if (changed) wiz.peak = p
        }
        function onButtonsChanged() {
            var m = agent.buttons, s = Object.assign({}, wiz.seenButtons)
            for (var i = 0; i < 64; i++) if (Math.floor(m / Math.pow(2, i)) % 2 === 1) s[i] = true
            wiz.seenButtons = s
        }
    }

    function finish() {
        var save = function() {
            agent.call("settings.set", {"settings": {"first_run_done": true}}, function(ok, r) {
                win.toast(ok ? qsTr("Setup complete") : r)
                wiz.close()
            })
        }
        if (unmatched !== "scroll") { save(); return }
        // Make Default scroll: copy the desktop preset's scroll tuning into it.
        agent.call("profiles.get", {"profile": "desktop"}, function(ok1, d) {
            agent.call("profiles.get", {"profile": "default"}, function(ok2, p) {
                if (!ok2) { win.toast(p); return }
                p.native_app_mode = "desktop-emulation"
                if (ok1 && d.desktop) p.desktop = d.desktop
                agent.call("profiles.save", {"profile": p}, function(ok3, r) { if (!ok3) win.toast(r); else save() })
            })
        })
    }

    component Check: RowLayout {
        property bool ok: false
        property bool warn: false
        property string text: ""
        spacing: 8
        Layout.fillWidth: true
        Label { text: ok ? "✔" : (warn ? "!" : "✖"); color: ok ? "#15803d" : (warn ? "#b45309" : "#b91c1c"); font.bold: true; Layout.alignment: Qt.AlignTop }
        Label { text: parent.text; wrapMode: Text.Wrap; Layout.fillWidth: true }
    }

    contentItem: ColumnLayout {
        spacing: 10
        Label { text: qsTr("Step %1 of %2").arg(wiz.step + 1).arg(wiz.lastStep + 1); opacity: 0.6 }
        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: wiz.step

            // 0 – identify the device
            ColumnLayout {
                spacing: 8
                Label { text: qsTr("Device"); font.bold: true; font.pointSize: 12 }
                Check { ok: agent.connected; text: agent.connected ? qsTr("Settings agent is running") : qsTr("The agent isn't running: systemctl --user start 3dmouse-agent") }
                Check { ok: wiz.b !== null; text: wiz.b ? qsTr("System service 3dmoused is running (mode: %1)").arg(wiz.b.mode) : qsTr("3dmoused isn't running: sudo systemctl enable --now 3dmoused") }
                Check {
                    ok: wiz.dev !== null && !wiz.dev.generic
                    warn: wiz.dev !== null && wiz.dev.generic
                    text: wiz.dev ? qsTr("Found %1 (%2), support status: %3").arg(wiz.dev.name).arg(wiz.dev.usb_id).arg(wiz.dev.support_status)
                                  : qsTr("No 3D mouse found. Plug it in (or replug it after installing).")
                }
                Label { visible: wiz.dev && wiz.dev.generic; text: qsTr("This model isn't in the registry yet and uses a generic mapping. Please report it."); wrapMode: Text.Wrap; Layout.fillWidth: true; color: "#b45309" }
                Item { Layout.fillHeight: true }
            }

            // 1 – desktop
            ColumnLayout {
                spacing: 8
                Label { text: qsTr("Desktop"); font.bold: true; font.pointSize: 12 }
                Label { text: wiz.ag ? qsTr("Session: %1 · %2").arg(wiz.ag.desktop.session_type || "?").arg(wiz.ag.desktop.current_desktop || "?") : "" }
                Check {
                    ok: wiz.st && wiz.st.agent && wiz.st.agent.capabilities ? wiz.st.agent.capabilities.focus_tracking : false
                    warn: true
                    text: wiz.ag ? qsTr("Per-application profiles: %1").arg(wiz.ag.desktop.provider_note) : ""
                }
                Label {
                    visible: wiz.ag && wiz.ag.desktop.provider === "gnome-extension" && !(wiz.st.agent.capabilities && wiz.st.agent.capabilities.focus_tracking)
                    text: qsTr("On GNOME, enable the bundled extension, then switch windows once:\ngnome-extensions enable 3dmouse-linux@joshshearer.github.io")
                    wrapMode: Text.Wrap; Layout.fillWidth: true
                }
                Check { ok: wiz.b ? wiz.b.injection_available : false; warn: true; text: wiz.b ? (wiz.b.injection_available ? qsTr("Button shortcuts and scrolling can reach the desktop") : qsTr("Desktop input unavailable: %1").arg(wiz.b.injection_reason)) : "" }
                Check { ok: wiz.ag ? !wiz.ag.locked : false; text: qsTr("Screen lock detection is active: nothing is sent to the desktop while it's locked") }
                Item { Layout.fillHeight: true }
            }

            // 2 – validate input
            ColumnLayout {
                spacing: 6
                Label { text: qsTr("Test the controls"); font.bold: true; font.pointSize: 12 }
                Label { text: qsTr("Move the cap firmly in each direction, then press each button once."); wrapMode: Text.Wrap; Layout.fillWidth: true }
                Repeater {
                    model: 6
                    RowLayout {
                        Layout.fillWidth: true
                        Label { text: wiz.peak[index] >= 0.5 ? "✔" : "·"; color: "#15803d"; font.bold: true; Layout.preferredWidth: 16 }
                        Label { text: wiz.axisNames[index]; Layout.preferredWidth: 150 }
                        ProgressBar { from: 0; to: 1; value: Math.min(1, wiz.peak[index]); Layout.fillWidth: true; Accessible.name: wiz.axisNames[index] }
                    }
                }
                Check {
                    ok: wiz.buttonsDone > 0
                    text: wiz.buttonsDone > 0 ? qsTr("Buttons pressed: %1").arg(Object.keys(wiz.seenButtons).map(function(k) { return Number(k) + 1 }).join(", "))
                                              : qsTr("No button pressed yet")
                }
                Label { visible: wiz.axesDone === 6 && wiz.buttonsDone > 0; text: qsTr("All controls work."); color: "#15803d" }
                Item { Layout.fillHeight: true }
            }

            // 3 – native applications
            ColumnLayout {
                spacing: 8
                Label { text: qsTr("3D applications"); font.bold: true; font.pointSize: 12 }
                Label { text: qsTr("Blender, FreeCAD, KiCad and OpenSCAD talk to the 3D mouse through the libspnav socket. Open one now to check the connection."); wrapMode: Text.Wrap; Layout.fillWidth: true }
                Check { ok: wiz.b ? !!wiz.b.spnav_socket : false; text: wiz.b && wiz.b.spnav_socket ? qsTr("libspnav socket: %1").arg(wiz.b.spnav_socket) : qsTr("No libspnav socket (another driver such as spacenavd owns the device)") }
                Check {
                    ok: wiz.b && wiz.b.spnav_clients ? wiz.b.spnav_clients.length > 0 : false
                    warn: true
                    text: wiz.b && wiz.b.spnav_clients && wiz.b.spnav_clients.length ? qsTr("Connected: %1").arg(wiz.b.spnav_clients.join(", ")) : qsTr("No 3D application connected yet (optional)")
                }
                Label { text: qsTr("OpenSCAD: enable Preferences → Axes → SpaceNav. Apps that were already open before the driver started need to be reopened."); wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.75 }
                Item { Layout.fillHeight: true }
            }

            // 4 – sensible default
            ColumnLayout {
                spacing: 8
                Label { text: qsTr("Applications without a profile"); font.bold: true; font.pointSize: 12 }
                Label { text: qsTr("Blender, FreeCAD, KiCad, OpenSCAD, PrusaSlicer and others have their own presets. What should every other application get?"); wrapMode: Text.Wrap; Layout.fillWidth: true }
                ButtonGroup { id: grp }
                RadioButton {
                    ButtonGroup.group: grp
                    checked: wiz.unmatched === "native"
                    text: qsTr("3D navigation only (recommended if you use other 3D programs)")
                    onToggled: if (checked) wiz.unmatched = "native"
                }
                RadioButton {
                    ButtonGroup.group: grp
                    checked: wiz.unmatched === "scroll"
                    text: qsTr("Scroll in browsers, editors and other ordinary apps")
                    onToggled: if (checked) wiz.unmatched = "scroll"
                }
                Label { text: qsTr("With scrolling, a 3D program without a preset receives scroll instead of 6-DOF motion until you give it a profile. You can change this any time under Profiles → Default."); wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.75 }
                Item { Layout.fillHeight: true }
            }

            // 5 – done
            ColumnLayout {
                spacing: 8
                Label { text: qsTr("Ready"); font.bold: true; font.pointSize: 12 }
                Check { ok: wiz.dev !== null; text: qsTr("Device detected") }
                Check { ok: wiz.axesDone === 6; warn: wiz.axesDone > 0; text: qsTr("%1 of 6 axes tested").arg(wiz.axesDone) }
                Check { ok: wiz.buttonsDone > 0; warn: true; text: qsTr("%1 button(s) tested").arg(wiz.buttonsDone) }
                Check { ok: true; text: wiz.unmatched === "scroll" ? qsTr("Other applications: scroll") : qsTr("Other applications: 3D navigation only") }
                Label { text: qsTr("Finish saves your choice. Run this again from Settings → Run setup."); wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.75 }
                Item { Layout.fillHeight: true }
            }
        }
    }

    footer: DialogButtonBox {
        Button { text: qsTr("Skip"); DialogButtonBox.buttonRole: DialogButtonBox.RejectRole; onClicked: agent.call("settings.set", {"settings": {"first_run_done": true}}, function() { wiz.close() }) }
        Button { text: qsTr("Back"); enabled: wiz.step > 0; onClicked: wiz.step-- }
        Button {
            text: wiz.step === wiz.lastStep ? qsTr("Finish") : qsTr("Next")
            highlighted: true
            onClicked: wiz.step === wiz.lastStep ? wiz.finish() : wiz.step++
        }
    }
}

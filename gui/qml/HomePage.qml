import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: page
    clip: true
    readonly property var st: agent.status
    readonly property var b: st && st.broker ? st.broker : null
    ColumnLayout {
        width: page.availableWidth
        spacing: 12
        anchors.margins: 16
        Item { height: 4 }
        Card {
            title: qsTr("Device")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Repeater {
                model: b && b.devices ? b.devices : []
                ColumnLayout {
                    Label { text: modelData.name + "  (" + modelData.usb_id + ")"; font.bold: true }
                    Label { text: qsTr("State: %1 · input via %2").arg(modelData.state).arg(modelData.input_path) }
                    Label {
                        text: qsTr("Support status: %1%2").arg(modelData.support_status).arg(modelData.generic ? qsTr(" (generic fallback – please report this device)") : "")
                        color: modelData.support_status === "unknown" || modelData.generic ? "#b45309" : palette.text
                    }
                    Label { visible: modelData.battery !== null && modelData.battery !== undefined; text: qsTr("Battery: %1%").arg(modelData.battery) }
                }
            }
            Label { visible: !b || !b.devices || b.devices.length === 0; text: qsTr("No 3D mouse detected. Connect the device or receiver.") }
        }
        Card {
            title: qsTr("Active profile")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Label { text: st && st.profile ? st.profile.display_name + "  [" + st.profile.id + "]" : "–"; font.bold: true }
            Label { text: st && st.profile ? qsTr("Selected by: %1 · output mode: %2").arg(st.profile.selected_by).arg(st.profile.app_mode) : "" }
            Label { text: st && st.agent ? qsTr("Focused application: %1").arg(st.agent.focus.app_id || "–") : "" }
            RowLayout {
                Button { text: qsTr("Automatic selection"); enabled: agent.connected; onClicked: agent.call("profiles.activate", {"profile": null, "persist": true}, function(ok, r) { win.toast(ok ? qsTr("Automatic profile selection") : r) }) }
            }
        }
        Card {
            title: qsTr("Applications")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Label { text: b ? qsTr("Ownership mode: %1 – %2").arg(b.mode).arg(b.mode_reason) : qsTr("Broker not connected"); wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label { text: b && b.spnav_socket ? qsTr("libspnav socket: %1").arg(b.spnav_socket) : ""; visible: text !== "" }
            Label { text: b ? qsTr("Connected 6-DOF applications: %1").arg(b.spnav_clients.length ? b.spnav_clients.join(", ") : qsTr("none")) : "" }
            Label { text: b ? qsTr("Desktop input: %1").arg(b.injection_available ? qsTr("available") : b.injection_reason) : ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        }
        Card {
            title: qsTr("Desktop")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Label { text: st && st.agent ? st.agent.desktop.session_type + " · " + st.agent.desktop.current_desktop : "" }
            Label { text: st && st.agent ? qsTr("Focus tracking: %1 (%2)").arg(st.agent.desktop.provider).arg(st.agent.desktop.provider_note) : ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        }
        Item { Layout.fillHeight: true }
    }
}

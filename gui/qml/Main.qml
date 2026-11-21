import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: win
    width: 1060
    height: 720
    minimumWidth: 760
    minimumHeight: 520
    visible: true
    title: qsTr("3DMouse Settings")

    readonly property var st: agent.status
    readonly property var brokerSt: st && st.broker ? st.broker : null
    readonly property var device: brokerSt && brokerSt.devices && brokerSt.devices.length > 0 ? brokerSt.devices[0] : null
    readonly property string activeProfile: st && st.profile ? st.profile.id : ""

    function toast(msg) { toastLabel.text = msg; toastTimer.restart() }
    function openSetup() { setup.open() }

    SetupWizard { id: setup }
    // first run: open the wizard once the agent answers, unless already done
    property bool setupChecked: false
    Connections {
        target: agent
        function onConnectedChanged() {
            if (!agent.connected || win.setupChecked || screenshotMode) return
            win.setupChecked = true
            agent.call("settings.get", {}, function(ok, s) { if (ok && !s.first_run_done) setup.open() })
        }
    }

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            Label { text: qsTr("3DMouse Settings"); font.bold: true; font.pointSize: 12 }
            Item { Layout.fillWidth: true }
            Label {
                text: !agent.connected ? qsTr("Agent offline")
                      : (win.device ? win.device.name + " · " + (st.profile ? st.profile.display_name : "") : qsTr("No device"))
                elide: Text.ElideRight
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0
        ListView {
            id: nav
            objectName: "nav"
            Layout.preferredWidth: 190
            Layout.fillHeight: true
            focus: true
            keyNavigationEnabled: true
            currentIndex: 0
            model: [qsTr("Overview"), qsTr("Device"), qsTr("Navigation"), qsTr("Buttons"), qsTr("Profiles"), qsTr("Diagnostics"), qsTr("Settings")]
            Accessible.name: qsTr("Sections")
            delegate: ItemDelegate {
                width: nav.width
                text: modelData
                highlighted: ListView.isCurrentItem
                onClicked: nav.currentIndex = index
                Accessible.name: modelData
            }
        }
        ToolSeparator { orientation: Qt.Vertical; Layout.fillHeight: true }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0
            Pane {
                Layout.fillWidth: true
                visible: !agent.connected || (agent.connected && brokerSt === null) || (st && st.agent && st.agent.locked)
                background: Rectangle { color: "#fff3cd" }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    color: "#664d03"
                    text: !agent.connected ? agent.error
                          : (st && st.agent && st.agent.locked) ? qsTr("Session locked – desktop input from the 3D mouse is paused.")
                          : qsTr("The system service (3dmoused) is not running: ") + (st && st.agent ? st.agent.broker_error : "")
                }
            }
            StackLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: nav.currentIndex
                HomePage {}
                DevicePage { active: nav.currentIndex === 1 }
                NavigationPage { active: nav.currentIndex === 2 }
                ButtonsPage { active: nav.currentIndex === 3 }
                ProfilesPage { active: nav.currentIndex === 4 }
                DiagnosticsPage {}
                SettingsPage { active: nav.currentIndex === 6 }
            }
        }
    }

    Rectangle {
        visible: toastTimer.running
        anchors.bottom: parent.bottom
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottomMargin: 20
        radius: 6
        color: "#323232"
        width: toastLabel.implicitWidth + 32
        height: toastLabel.implicitHeight + 16
        Label { id: toastLabel; anchors.centerIn: parent; color: "white" }
        Timer { id: toastTimer; interval: 3000 }
        Accessible.role: Accessible.AlertMessage
        Accessible.name: toastLabel.text
    }
}

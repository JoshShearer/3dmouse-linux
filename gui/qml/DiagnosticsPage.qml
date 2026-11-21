import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Diagnostics (FR-900 GUI side): health checks and support bundle.
ScrollView {
    id: page
    clip: true
    property string output: ""
    ColumnLayout {
        width: page.availableWidth
        spacing: 12
        Item { height: 4 }
        RowLayout {
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Button { text: qsTr("Run checks"); onClicked: { page.output = qsTr("Running…"); agent.runCtl(["diagnostics", "check"], function(ok, out) { page.output = out }) } }
            Button {
                text: qsTr("Create support bundle")
                onClicked: {
                    var path = agent.status && agent.status.agent ? agent.status.agent.config_dir + "/3dmouse-support.json" : "3dmouse-support.json";
                    agent.runCtl(["support-bundle", "--redact", "--out", path], function(ok, out) { page.output = ok ? qsTr("Wrote %1 – serial numbers are hashed, window titles are never collected. Review it before sharing.").arg(path) : out })
                }
            }
            Button { text: qsTr("Show raw status"); onClicked: page.output = agent.toJson(agent.status) }
        }
        TextArea {
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Layout.fillWidth: true
            Layout.preferredHeight: 520
            readOnly: true
            wrapMode: TextEdit.Wrap
            font.family: "monospace"
            text: page.output
            selectByMouse: true
            Accessible.name: qsTr("Diagnostics output")
        }
    }
}

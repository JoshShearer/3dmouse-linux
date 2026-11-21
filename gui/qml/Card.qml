import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Titled group box used across pages.
Frame {
    id: card
    property string title: ""
    default property alias content: col.data
    Layout.fillWidth: true
    Layout.minimumWidth: 0 // let long content wrap instead of widening the page
    padding: 14
    ColumnLayout {
        id: col
        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 8
        Label {
            visible: card.title !== ""
            text: card.title
            font.bold: true
            font.pointSize: 11
            Accessible.role: Accessible.Heading
        }
    }
}

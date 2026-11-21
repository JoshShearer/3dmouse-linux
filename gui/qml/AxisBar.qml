import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Centre-zero bar for one axis; value in [-1, 1].
RowLayout {
    property string label
    property real value: 0
    property string text: ""
    spacing: 8
    Accessible.name: label + " " + text
    Label { text: parent.label; Layout.preferredWidth: 28; font.family: "monospace" }
    Rectangle {
        Layout.fillWidth: true
        height: 16
        radius: 3
        color: palette.base
        border.color: palette.mid
        Rectangle {
            readonly property real v: Math.max(-1, Math.min(1, parent.parent.value))
            x: v >= 0 ? parent.width / 2 : parent.width / 2 * (1 + v)
            width: Math.abs(v) * parent.width / 2
            height: parent.height
            radius: 3
            color: palette.highlight
        }
        Rectangle { x: parent.width / 2; width: 1; height: parent.height; color: palette.mid }
    }
    Label { text: parent.text; Layout.preferredWidth: 60; horizontalAlignment: Text.AlignRight; font.family: "monospace" }
}

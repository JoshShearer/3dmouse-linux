import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Live view (FR-801): raw and processed axes, buttons, 3-D preview.
ScrollView {
    id: page
    property bool active: false
    clip: true
    onActiveChanged: agent.live = active
    readonly property var dev: win.device
    readonly property var names: ["TX", "TY", "TZ", "RX", "RY", "RZ"]
    readonly property real range: dev && dev.axis_range > 0 ? dev.axis_range : 350

    ColumnLayout {
        width: page.availableWidth
        spacing: 12
        Item { height: 4 }
        RowLayout {
            Layout.leftMargin: 16; Layout.rightMargin: 16
            spacing: 12
            Card {
                title: qsTr("Raw input (canonical frame)")
                Layout.preferredWidth: 1
                Repeater {
                    model: 6
                    AxisBar { Layout.fillWidth: true; label: page.names[index]; value: agent.raw[index] / page.range; text: agent.raw[index] }
                }
            }
            Card {
                title: qsTr("Output to applications")
                Layout.preferredWidth: 1
                Repeater {
                    model: 6
                    AxisBar { Layout.fillWidth: true; label: page.names[index]; value: agent.norm[index]; text: Number(agent.norm[index]).toFixed(2) }
                }
            }
        }
        RowLayout {
            Layout.leftMargin: 16; Layout.rightMargin: 16
            spacing: 12
            Card {
                title: qsTr("Preview")
                Layout.preferredWidth: 1
                Canvas {
                    id: cube
                    Layout.fillWidth: true
                    Layout.preferredHeight: 260
                    Accessible.name: qsTr("3D preview of the current motion")
                    property var n: agent.norm
                    onNChanged: requestPaint()
                    onPaint: {
                        var ctx = getContext("2d");
                        ctx.reset();
                        ctx.fillStyle = palette.base; ctx.fillRect(0, 0, width, height);
                        var s = Math.min(width, height) * 0.22;
                        // canonical frame: X right, Y up, Z away from the user
                        var tx = n[0] * s * 0.6, ty = -n[1] * s * 0.6, tz = n[2];
                        var ax = n[3] * 0.6, ay = n[4] * 0.6, az = n[5] * 0.6;
                        var pts = [[-1,-1,-1],[1,-1,-1],[1,1,-1],[-1,1,-1],[-1,-1,1],[1,-1,1],[1,1,1],[-1,1,1]];
                        var P = [];
                        for (var i = 0; i < 8; i++) {
                            var x = pts[i][0], y = pts[i][1], z = pts[i][2], t;
                            t = y * Math.cos(ax) - z * Math.sin(ax); z = y * Math.sin(ax) + z * Math.cos(ax); y = t;
                            t = x * Math.cos(ay) + z * Math.sin(ay); z = -x * Math.sin(ay) + z * Math.cos(ay); x = t;
                            t = x * Math.cos(az) - y * Math.sin(az); y = x * Math.sin(az) + y * Math.cos(az); x = t;
                            // light perspective tilt so the cube reads as 3-D
                            var ry = 0.5, rx = 0.35;
                            t = x * Math.cos(ry) + z * Math.sin(ry); z = -x * Math.sin(ry) + z * Math.cos(ry); x = t;
                            t = y * Math.cos(rx) - z * Math.sin(rx); z = y * Math.sin(rx) + z * Math.cos(rx); y = t;
                            var k = 4 / (4 + z + tz * 1.5);
                            P.push([width / 2 + tx + x * s * k, height / 2 + ty - y * s * k]);
                        }
                        var E = [[0,1],[1,2],[2,3],[3,0],[4,5],[5,6],[6,7],[7,4],[0,4],[1,5],[2,6],[3,7]];
                        ctx.strokeStyle = palette.highlight; ctx.lineWidth = 2;
                        ctx.beginPath();
                        for (var e = 0; e < E.length; e++) { ctx.moveTo(P[E[e][0]][0], P[E[e][0]][1]); ctx.lineTo(P[E[e][1]][0], P[E[e][1]][1]); }
                        ctx.stroke();
                    }
                }
            }
            Card {
                title: qsTr("Buttons")
                Layout.preferredWidth: 1
                Layout.alignment: Qt.AlignTop
                Flow {
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: page.dev ? page.dev.buttons : []
                        Rectangle {
                            readonly property bool down: (agent.buttons / Math.pow(2, modelData.index)) % 2 >= 1
                            width: lbl.implicitWidth + 24; height: 36; radius: 6
                            color: down ? palette.highlight : palette.button
                            border.color: palette.mid
                            Accessible.name: modelData.label + (down ? qsTr(" pressed") : "")
                            Label { id: lbl; anchors.centerIn: parent; text: modelData.label; color: parent.down ? palette.highlightedText : palette.buttonText }
                        }
                    }
                }
                Label {
                    text: page.dev ? qsTr("%1 · %2 Hz · %3 frames · %4 malformed · %5 watchdog resets")
                                     .arg(page.dev.input_path).arg(Math.round(page.dev.event_rate_hz)).arg(page.dev.frames).arg(page.dev.malformed).arg(page.dev.watchdog_resets) : qsTr("No device")
                    wrapMode: Text.Wrap; Layout.fillWidth: true
                }
                Label { text: qsTr("Serial (redacted): %1").arg(page.dev ? page.dev.serial : "–"); opacity: 0.7 }
            }
        }
        Item { Layout.fillHeight: true }
    }
}

import QtQuick
import QtQuick.Window

// Overlay windows for 3dmouse-osd. Both start hidden; C++ turns them into
// layer-shell overlay surfaces (or X11 always-on-top windows) before showing.
QtObject {
    id: root
    property var settings: ({})
    readonly property real scale: Math.max(0.75, Math.min(2.0, Number(settings.osd_scale) || 1.0))
    readonly property string position: ["top", "center", "bottom"].indexOf(settings.osd_position) >= 0 ? settings.osd_position : "bottom"

    // radial state from the agent
    property var items: []
    property int hover: -1
    property int chosen: -1

    function loadSettings(then) {
        agent.call("settings.get", {}, function(ok, s) { if (ok) root.settings = s; if (then) then() })
    }
    function register() {
        if (agent.connected) agent.call("osd.register", {}, function() {})
        loadSettings()
    }

    Component.onCompleted: if (agent.connected) register()

    property Connections conn: Connections {
        target: agent
        function onConnectedChanged() { if (agent.connected) root.register(); else { toast.visible = false; radial.visible = false } }
        function onAgentEvent(name, body) {
            if (name === "osd") {
                root.loadSettings(function() { toast.show(body.text, body.timeout_ms || 1500) })
            } else if (name === "radial") {
                if (body.state === "open") {
                    root.items = body.items || []; root.hover = -1; root.chosen = -1
                    radial.visible = true
                    placer.place(radial, "center", 0)
                    ring.requestPaint()
                } else if (body.state === "hover") {
                    root.hover = body.hover; ring.requestPaint()
                } else if (body.state === "close") {
                    root.chosen = (body.chosen === null || body.chosen === undefined) ? -1 : body.chosen
                    ring.requestPaint()
                    closeTimer.restart()
                }
            } else if (name === "lock" && body.locked) {
                toast.visible = false; radial.visible = false
            }
        }
    }
    property Timer closeTimer: Timer { interval: 250; onTriggered: radial.visible = false }

    property Window toast: Window {
        id: toast
        objectName: "osdToast"
        color: "transparent"
        visible: false
        width: Math.ceil(Math.min(640 * root.scale, Math.max(220 * root.scale, msg.implicitWidth + 40 * root.scale)))
        height: Math.ceil(msg.implicitHeight + 28 * root.scale)
        function show(text, ms) {
            msg.text = text
            visible = true
            placer.place(toast, root.position, 48)
            hideTimer.interval = Math.max(500, Math.min(10000, ms))
            hideTimer.restart()
        }
        Timer { id: hideTimer; onTriggered: toast.visible = false }
        Rectangle {
            anchors.fill: parent
            radius: 12 * root.scale
            color: "#e6202124"
            border.color: "#40ffffff"
            Text {
                id: msg
                anchors.centerIn: parent
                width: Math.min(implicitWidth, 600 * root.scale)
                color: "white"
                font.pixelSize: 16 * root.scale
                wrapMode: Text.Wrap
                horizontalAlignment: Text.AlignHCenter
            }
        }
    }

    property Window radial: Window {
        id: radial
        objectName: "osdRadial"
        color: "transparent"
        visible: false
        width: Math.ceil(420 * root.scale)
        height: width
        Canvas {
            id: ring
            anchors.fill: parent
            onPaint: {
                var ctx = getContext("2d")
                ctx.reset()
                var n = root.items.length
                if (n < 1) return
                var c = width / 2, ro = width / 2 - 4, ri = ro * 0.38
                var w = 2 * Math.PI / n
                for (var i = 0; i < n; i++) {
                    // slice i is centred on 12 o'clock + i*w, clockwise (matches the agent)
                    var a0 = -Math.PI / 2 + i * w - w / 2, a1 = a0 + w
                    ctx.beginPath()
                    ctx.arc(c, c, ro, a0, a1, false)
                    ctx.arc(c, c, ri, a1, a0, true)
                    ctx.closePath()
                    var sel = i === root.chosen, hov = i === root.hover
                    ctx.fillStyle = sel ? "#f02563eb" : (hov ? "#e03b82f6" : "#e0202124")
                    ctx.fill()
                    ctx.lineWidth = 2
                    ctx.strokeStyle = "#50ffffff"
                    ctx.stroke()
                }
                ctx.beginPath()
                ctx.arc(c, c, ri - 6, 0, 2 * Math.PI)
                ctx.fillStyle = "#c0101012"
                ctx.fill()
            }
        }
        Repeater {
            model: root.items
            Text {
                readonly property real ang: -Math.PI / 2 + index * 2 * Math.PI / Math.max(1, root.items.length)
                readonly property real rr: radial.width / 2 * 0.69
                x: radial.width / 2 + rr * Math.cos(ang) - width / 2
                y: radial.height / 2 + rr * Math.sin(ang) - height / 2
                width: radial.width * 0.27
                text: modelData
                color: "white"
                font.pixelSize: 14 * root.scale
                font.bold: index === root.hover
                wrapMode: Text.Wrap
                horizontalAlignment: Text.AlignHCenter
            }
        }
        Text {
            anchors.centerIn: parent
            width: radial.width * 0.26
            text: root.hover >= 0 ? qsTr("Release to choose") : qsTr("Push the cap")
            color: "#c0ffffff"
            font.pixelSize: 12 * root.scale
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
        }
    }
}

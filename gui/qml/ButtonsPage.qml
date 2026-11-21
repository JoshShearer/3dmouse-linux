import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Button assignment (FR-301..FR-308).
ScrollView {
    id: page
    property bool active: false
    property var prof: null
    property bool dirty: false
    property var profileIds: []
    // conflict and "won't work" findings for the working copy (agent profiles.lint)
    property var lints: []
    onProfChanged: relint()
    function relint() {
        if (!prof) { lints = []; return }
        agent.call("profiles.lint", {"profile": prof}, function(ok, r) { page.lints = ok ? r : [] })
    }
    function lintsFor(i) { return lints.filter(function(l) { return l.button === i }) }
    clip: true
    readonly property var buttons: win.device ? win.device.buttons : [{"index": 0, "label": qsTr("Button 1")}, {"index": 1, "label": qsTr("Button 2")}]

    function load() {
        if (!win.activeProfile) return;
        agent.call("profiles.get", {"profile": win.activeProfile}, function(ok, p) { if (ok) { page.prof = p; page.dirty = false } })
        agent.call("profiles.list", {}, function(ok, l) { if (ok) page.profileIds = l.map(function(p) { return p.id }) })
    }
    function binding(i) {
        if (!prof || !prof.bindings) return null;
        return prof.bindings[String(i)] || null;
    }
    function setBinding(i, slot, action) {
        var p = JSON.parse(JSON.stringify(prof));
        if (!p.bindings) p.bindings = {};
        var b = p.bindings[String(i)] || {"tap": {"type": "spnav_button", "button": i}};
        if (action === null) delete b[slot]; else b[slot] = action;
        p.bindings[String(i)] = b;
        prof = p;
        dirty = true;
    }
    function resetBinding(i) {
        var p = JSON.parse(JSON.stringify(prof));
        if (p.bindings) delete p.bindings[String(i)];
        prof = p; dirty = true;
    }
    onActiveChanged: { agent.live = active; if (active && !dirty) load() }
    Connections { target: win; function onActiveProfileChanged() { if (page.active && !page.dirty) page.load() } }

    ColumnLayout {
        width: page.availableWidth
        spacing: 12
        Item { height: 4 }
        RowLayout {
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Label { text: page.prof ? qsTr("Profile: %1 – press a device button to find it below").arg(page.prof.display_name) : qsTr("Loading…"); font.bold: true; Layout.fillWidth: true; wrapMode: Text.Wrap }
            Button { text: qsTr("Revert"); enabled: page.dirty; onClicked: page.load() }
            Button {
                text: qsTr("Save"); enabled: page.dirty; highlighted: true
                onClicked: agent.call("profiles.save", {"profile": page.prof}, function(ok, r) { win.toast(ok ? qsTr("Saved") : r); if (ok) page.dirty = false })
            }
        }
        Repeater {
            model: page.buttons
            Card {
                Layout.leftMargin: 16; Layout.rightMargin: 16
                readonly property int bi: modelData.index
                readonly property bool down: (agent.buttons / Math.pow(2, bi)) % 2 >= 1
                title: modelData.label + (down ? qsTr("  ● pressed") : "")
                readonly property var b: page.binding(bi)
                GridLayout {
                    columns: 2
                    Layout.fillWidth: true
                    Label { text: qsTr("Press"); Layout.alignment: Qt.AlignTop }
                    ActionEditor {
                        Layout.fillWidth: true
                        profileIds: page.profileIds
                        action: b ? b.tap : {"type": "spnav_button", "button": bi}
                        onEdited: function(a) { page.setBinding(bi, "tap", a) }
                    }
                    CheckBox { id: holdOn; text: qsTr("Hold"); checked: b && b.hold ? true : false; onToggled: page.setBinding(bi, "hold", checked ? {"type": "show_help"} : null) }
                    ActionEditor {
                        visible: holdOn.checked
                        Layout.fillWidth: true
                        profileIds: page.profileIds
                        action: b && b.hold ? b.hold : {"type": "none"}
                        onEdited: function(a) { page.setBinding(bi, "hold", a) }
                    }
                    Item { visible: !holdOn.checked; Layout.fillWidth: true; height: 1 }
                    CheckBox { id: dblOn; text: qsTr("Double press"); checked: b && b.double ? true : false; onToggled: page.setBinding(bi, "double", checked ? {"type": "profile_cycle"} : null) }
                    ActionEditor {
                        visible: dblOn.checked
                        Layout.fillWidth: true
                        profileIds: page.profileIds
                        action: b && b.double ? b.double : {"type": "none"}
                        onEdited: function(a) { page.setBinding(bi, "double", a) }
                    }
                    Item { visible: !dblOn.checked; Layout.fillWidth: true; height: 1 }
                }
                Repeater {
                    model: page.lintsFor(bi)
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        color: modelData.severity === "error" ? "#d93025" : "#b06000"
                        text: (modelData.severity === "error" ? "✖ " : "⚠ ") + (modelData.slot ? modelData.slot + ": " : "") + modelData.message
                        Accessible.role: Accessible.AlertMessage
                        Accessible.name: text
                    }
                }
                RowLayout {
                    Button { text: qsTr("Test"); enabled: b !== null && b.tap.type !== "spnav_button"; onClicked: agent.call("test_action", {"action": b.tap}, function(ok, r) { if (!ok) win.toast(r) }) }
                    Button { text: qsTr("Reset to application button"); onClicked: page.resetBinding(bi) }
                }
            }
        }
        Card {
            title: qsTr("Timing")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            visible: page.prof !== null
            RowLayout {
                Label { text: qsTr("Hold after") }
                SpinBox { Accessible.name: qsTr("Hold after, milliseconds"); from: 150; to: 2000; stepSize: 50; value: page.prof && page.prof.timing ? page.prof.timing.hold_ms : 450; onValueModified: { var p = JSON.parse(JSON.stringify(page.prof)); p.timing = p.timing || {}; p.timing.hold_ms = value; page.prof = p; page.dirty = true } }
                Label { text: qsTr("ms · double-press window") }
                SpinBox { Accessible.name: qsTr("Double-press window, milliseconds"); from: 100; to: 1000; stepSize: 25; value: page.prof && page.prof.timing ? page.prof.timing.double_ms : 250; onValueModified: { var p = JSON.parse(JSON.stringify(page.prof)); p.timing = p.timing || {}; p.timing.double_ms = value; page.prof = p; page.dirty = true } }
                Label { text: qsTr("ms") }
            }
            Label { text: qsTr("Buttons with a hold or double-press action are delayed slightly while the agent decides which one you meant."); wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.7 }
        }
        Item { Layout.fillHeight: true }
    }
}

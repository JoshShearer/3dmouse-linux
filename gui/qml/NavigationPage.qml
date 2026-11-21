import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Navigation tuning (FR-202..FR-207). Edits preview live on the device and
// are only written to disk with "Save".
ScrollView {
    id: page
    property bool active: false
    property var prof: null
    property bool dirty: false
    clip: true
    readonly property var names: ["TX (left/right)", "TY (up/down)", "TZ (push/pull)", "RX (tilt fwd/back)", "RY (twist)", "RZ (tilt left/right)"]

    // Which profile is being edited: follows the active profile until the
    // user picks one explicitly (so Blender's profile can be tuned while
    // this window has focus).
    property string pinned: ""
    property var list: []
    readonly property string editId: pinned !== "" ? pinned : win.activeProfile
    readonly property bool isActive: prof !== null && prof.id === win.activeProfile

    function load() {
        agent.call("profiles.list", {}, function(ok, l) { if (ok) page.list = l })
        if (!page.editId) return;
        agent.call("profiles.get", {"profile": page.editId}, function(ok, p) { if (ok) { page.prof = p; page.dirty = false } })
    }
    function nav() { return prof ? prof.navigation : null }
    function changed() {
        dirty = true;
        prof = prof; // re-evaluate bindings
        previewTimer.restart();
    }
    onActiveChanged: { agent.live = active; if (active && !dirty) load() }
    Connections { target: win; function onActiveProfileChanged() { if (page.active && !page.dirty && page.pinned === "") page.load() } }
    // live preview only touches the running profile, never another one
    Timer { id: previewTimer; interval: 60; onTriggered: if (page.isActive) agent.call("preview_nav", {"nav": page.prof.navigation}, function(ok, r) { if (!ok) win.toast(r) }) }

    ColumnLayout {
        width: page.availableWidth
        spacing: 12
        Item { height: 4 }
        RowLayout {
            Layout.leftMargin: 16; Layout.rightMargin: 16
            Label { text: qsTr("Editing profile"); font.bold: true }
            ComboBox {
                id: profPick
                Layout.preferredWidth: 260
                model: page.list
                textRole: "display_name"
                valueRole: "id"
                currentIndex: { for (var i = 0; i < page.list.length; ++i) if (page.list[i].id === page.editId) return i; return -1 }
                Accessible.name: qsTr("Profile to edit")
                onActivated: {
                    if (page.dirty) { win.toast(qsTr("Save or revert your changes first")); currentIndex = Qt.binding(function() { for (var i = 0; i < page.list.length; ++i) if (page.list[i].id === page.editId) return i; return -1 }); return }
                    page.pinned = currentValue === win.activeProfile ? "" : currentValue;
                    page.load()
                }
            }
            Label {
                Layout.fillWidth: true; opacity: 0.7; wrapMode: Text.Wrap
                text: page.isActive ? qsTr("Active now – changes preview live on the device") : qsTr("Not active – changes apply when this profile is used")
            }
            Button { text: qsTr("Revert"); enabled: page.dirty; onClicked: { page.load(); agent.call("profiles.reload") } }
            Button {
                text: qsTr("Save"); enabled: page.dirty; highlighted: true
                onClicked: agent.call("profiles.save", {"profile": page.prof}, function(ok, r) { win.toast(ok ? qsTr("Saved") : r); if (ok) page.dirty = false })
            }
        }
        Card {
            title: qsTr("Sensitivity")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            visible: page.prof !== null
            GridLayout {
                columns: 3
                Layout.fillWidth: true
                Label { text: qsTr("Overall speed") }
                Slider { id: mg; Layout.fillWidth: true; from: 0.05; to: 5; value: page.nav() ? page.nav().master_gain : 1; Accessible.name: qsTr("Overall speed"); onMoved: { page.prof.navigation.master_gain = value; page.changed() } }
                Label { text: mg.value.toFixed(2) + "×"; Layout.preferredWidth: 50 }
                Label { text: qsTr("Pan / zoom speed") }
                Slider { id: tg; Layout.fillWidth: true; from: 0; to: 5; value: page.nav() ? page.nav().translation_gain : 1; Accessible.name: qsTr("Translation speed"); onMoved: { page.prof.navigation.translation_gain = value; page.changed() } }
                Label { text: tg.value.toFixed(2) + "×" }
                Label { text: qsTr("Rotation speed") }
                Slider { id: rg; Layout.fillWidth: true; from: 0; to: 5; value: page.nav() ? page.nav().rotation_gain : 1; Accessible.name: qsTr("Rotation speed"); onMoved: { page.prof.navigation.rotation_gain = value; page.changed() } }
                Label { text: rg.value.toFixed(2) + "×" }
            }
        }
        Card {
            title: qsTr("Response")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            visible: page.prof !== null
            GridLayout {
                columns: 3
                Layout.fillWidth: true
                Label { text: qsTr("Curve") }
                ComboBox {
                    id: curve
                    Layout.fillWidth: true
                    model: [qsTr("Linear"), qsTr("Power (fine control near centre)"), qsTr("S-curve")]
                    readonly property var kinds: ["linear", "power", "s_curve"]
                    currentIndex: page.nav() ? Math.max(0, kinds.indexOf(page.nav().response_curve.kind)) : 0
                    Accessible.name: qsTr("Response curve")
                    onActivated: {
                        var k = kinds[currentIndex];
                        page.prof.navigation.response_curve = k === "power" ? {"kind": "power", "exponent": 2.0} : k === "s_curve" ? {"kind": "s_curve", "k": 0.5} : {"kind": "linear"};
                        page.changed()
                    }
                }
                Item {}
                Label { text: qsTr("Curve strength"); visible: curve.currentIndex > 0 }
                Slider {
                    id: cs
                    visible: curve.currentIndex > 0
                    Layout.fillWidth: true
                    from: curve.currentIndex === 1 ? 1 : 0; to: curve.currentIndex === 1 ? 4 : 1
                    value: !page.nav() ? 0 : (page.nav().response_curve.kind === "power" ? page.nav().response_curve.exponent : (page.nav().response_curve.k || 0))
                    Accessible.name: qsTr("Curve strength")
                    onMoved: {
                        if (curve.currentIndex === 1) page.prof.navigation.response_curve = {"kind": "power", "exponent": value};
                        else page.prof.navigation.response_curve = {"kind": "s_curve", "k": value};
                        page.changed()
                    }
                }
                Label { visible: curve.currentIndex > 0; text: cs.value.toFixed(2) }
                Label { text: qsTr("Smoothing") }
                Slider { id: sm; Layout.fillWidth: true; from: 0; to: 300; stepSize: 5; value: page.nav() ? page.nav().smoothing_time_ms : 0; Accessible.name: qsTr("Smoothing time in milliseconds"); onMoved: { page.prof.navigation.smoothing_time_ms = value; page.changed() } }
                Label { text: sm.value.toFixed(0) + " ms" }
            }
            RowLayout {
                CheckBox { text: qsTr("Pan/zoom"); checked: page.nav() ? page.nav().translation_enabled : true; onToggled: { page.prof.navigation.translation_enabled = checked; page.changed() } }
                CheckBox { text: qsTr("Rotation"); checked: page.nav() ? page.nav().rotation_enabled : true; onToggled: { page.prof.navigation.rotation_enabled = checked; page.changed() } }
                CheckBox { text: qsTr("Dominant axis only"); checked: page.nav() ? page.nav().dominant_axis : false; onToggled: { page.prof.navigation.dominant_axis = checked; page.changed() } }
            }
        }
        Card {
            title: qsTr("Axes")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            visible: page.prof !== null
            Label { text: qsTr("For each output axis: source axis, speed, dead zone, invert, enabled."); opacity: 0.7 }
            Repeater {
                model: 6
                RowLayout {
                    Layout.fillWidth: true
                    readonly property int ax: index
                    Label { text: page.names[ax]; Layout.preferredWidth: 150 }
                    ComboBox {
                        Layout.preferredWidth: 110
                        model: ["TX", "TY", "TZ", "RX", "RY", "RZ"]
                        currentIndex: page.nav() ? page.nav().axis_map[ax] : ax
                        Accessible.name: qsTr("Source axis for %1").arg(page.names[ax])
                        onActivated: { var m = page.prof.navigation.axis_map.slice(); m[ax] = currentIndex; page.prof.navigation.axis_map = m; page.changed() }
                    }
                    Slider {
                        Layout.fillWidth: true; from: 0; to: 3
                        value: page.nav() ? page.nav().per_axis_gain[ax] : 1
                        Accessible.name: qsTr("Speed for %1").arg(page.names[ax])
                        onMoved: { var g = page.prof.navigation.per_axis_gain.slice(); g[ax] = value; page.prof.navigation.per_axis_gain = g; page.changed() }
                    }
                    Label { text: page.nav() ? Number(page.nav().per_axis_gain[ax]).toFixed(2) : ""; Layout.preferredWidth: 40 }
                    SpinBox {
                        from: 0; to: 50; editable: true
                        value: page.nav() ? Math.round(page.nav().deadzone[ax] * 100) : 2
                        textFromValue: function(v) { return v + " %" }
                        valueFromText: function(t) { return parseInt(t) }
                        Accessible.name: qsTr("Dead zone for %1 in percent").arg(page.names[ax])
                        onValueModified: { var d = page.prof.navigation.deadzone.slice(); d[ax] = value / 100; page.prof.navigation.deadzone = d; page.changed() }
                    }
                    CheckBox {
                        text: qsTr("Invert")
                        checked: page.nav() ? page.nav().per_axis_invert[ax] : false
                        Accessible.name: qsTr("Invert %1").arg(page.names[ax])
                        onToggled: { var v = page.prof.navigation.per_axis_invert.slice(); v[ax] = checked; page.prof.navigation.per_axis_invert = v; page.changed() }
                    }
                    CheckBox {
                        text: qsTr("On")
                        checked: page.nav() ? page.nav().per_axis_enabled[ax] : true
                        Accessible.name: qsTr("Enable %1").arg(page.names[ax])
                        onToggled: { var v = page.prof.navigation.per_axis_enabled.slice(); v[ax] = checked; page.prof.navigation.per_axis_enabled = v; page.changed() }
                    }
                }
            }
            Label { text: page.isActive ? qsTr("Tip: watch your application while adjusting – changes preview immediately; Save keeps them.") : qsTr("This profile is not active, so changes are saved but not previewed. Focus its application (or activate it on the Profiles page) to feel them live."); wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.7 }
        }
        Card {
            title: qsTr("Calibration")
            Layout.leftMargin: 16; Layout.rightMargin: 16
            visible: page.prof !== null
            Label { text: qsTr("If the cap drifts at rest, measure its resting offset. Do not touch the device while measuring."); wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label { text: page.nav() ? qsTr("Current offset: %1").arg(JSON.stringify(page.nav().center_offset)) : "" }
            RowLayout {
                Button {
                    text: qsTr("Measure and save (3 s)")
                    onClicked: { win.toast(qsTr("Measuring – hands off…")); agent.runCtl(["calibrate", "--seconds", "3", "--apply"], function(ok, out) { win.toast(ok ? qsTr("Calibration saved") : out); page.load() }) }
                }
                Button { text: qsTr("Clear offset"); onClicked: { page.prof.navigation.center_offset = [0, 0, 0, 0, 0, 0]; page.changed() } }
            }
        }
        Item { Layout.fillHeight: true }
    }
}

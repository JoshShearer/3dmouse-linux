import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Edits one action object (sm-actions Action, tagged by "type").
ColumnLayout {
    id: ed
    property var action: ({"type": "none"})
    property bool allowNone: true
    property var profileIds: []
    // radial menus can't be nested: item editors are created with allowRadial: false
    property bool allowRadial: true
    signal edited(var action)

    readonly property var allTypes: ["none", "spnav_button", "key", "mouse", "scroll", "view", "toggle", "sensitivity", "sensitivity_reset", "profile_switch", "profile_cycle", "launch", "exec", "macro", "show_help", "radial"]
    readonly property var allTypeNames: [qsTr("Nothing"), qsTr("Application button (native 6-DOF)"), qsTr("Keyboard shortcut"), qsTr("Mouse button"), qsTr("Scroll"), qsTr("View preset"), qsTr("Toggle"), qsTr("Change sensitivity"), qsTr("Reset sensitivity"), qsTr("Switch profile"), qsTr("Next profile"), qsTr("Launch application"), qsTr("Run command"), qsTr("Macro"), qsTr("Show button help"), qsTr("Radial menu (hold, push the cap, release)")]
    readonly property var types: allowRadial ? allTypes : allTypes.slice(0, allTypes.length - 1)
    readonly property var typeNames: allowRadial ? allTypeNames : allTypeNames.slice(0, allTypeNames.length - 1)

    function emitWith(patch) {
        var a = {};
        for (var k in patch) a[k] = patch[k];
        edited(a);
    }
    function defaults(t) {
        switch (t) {
        case "spnav_button": return {"type": t, "button": 0};
        case "key": return {"type": t, "keys": "ctrl+z"};
        case "mouse": return {"type": t, "button": "middle"};
        case "scroll": return {"type": t, "dx": 0, "dy": 1};
        case "view": return {"type": t, "view": "fit"};
        case "toggle": return {"type": t, "target": "rotation"};
        case "sensitivity": return {"type": t, "factor": 1.5};
        case "profile_switch": return {"type": t, "profile": "default"};
        case "launch": return {"type": t, "desktop_id": "org.freecad.FreeCAD"};
        case "exec": return {"type": t, "argv": [], "confirmed": false};
        case "macro": return {"type": t, "steps": [{"type": "key", "keys": "ctrl+c"}, {"type": "delay", "ms": 100}, {"type": "key", "keys": "ctrl+v"}]};
        case "radial": return {"type": t, "items": [
            {"label": qsTr("Undo"), "action": {"type": "key", "keys": "ctrl+z"}},
            {"label": qsTr("Fit view"), "action": {"type": "view", "view": "fit"}},
            {"label": qsTr("Redo"), "action": {"type": "key", "keys": "ctrl+shift+z"}},
            {"label": qsTr("Next profile"), "action": {"type": "profile_cycle"}}]};
        default: return {"type": t};
        }
    }
    function copy(extra) {
        var a = JSON.parse(JSON.stringify(action));
        for (var k in extra) a[k] = extra[k];
        return a;
    }

    ComboBox {
        id: typeBox
        Layout.fillWidth: true
        model: ed.typeNames
        currentIndex: Math.max(0, ed.types.indexOf(ed.action ? ed.action.type : "none"))
        Accessible.name: qsTr("Action type")
        onActivated: ed.edited(ed.defaults(ed.types[currentIndex]))
    }
    // --- parameters
    SpinBox {
        visible: ed.action && ed.action.type === "spnav_button"
        from: 1; to: 64
        value: ed.action && ed.action.button !== undefined ? ed.action.button + 1 : 1
        Accessible.name: qsTr("Application button number")
        onValueModified: ed.edited(ed.copy({"button": value - 1}))
    }
    TextField {
        visible: ed.action && ed.action.type === "key"
        Layout.fillWidth: true
        text: ed.action && ed.action.keys ? ed.action.keys : ""
        placeholderText: qsTr("e.g. ctrl+shift+z, f5, kp7")
        Accessible.name: qsTr("Keyboard shortcut")
        onEditingFinished: if (text !== ed.action.keys) ed.edited(ed.copy({"keys": text}))
    }
    ComboBox {
        visible: ed.action && ed.action.type === "mouse"
        model: ["left", "right", "middle", "back", "forward"]
        currentIndex: Math.max(0, model.indexOf(ed.action ? ed.action.button : "middle"))
        Accessible.name: qsTr("Mouse button")
        onActivated: ed.edited(ed.copy({"button": model[currentIndex]}))
    }
    RowLayout {
        visible: ed.action && ed.action.type === "scroll"
        Label { text: qsTr("Vertical") }
        SpinBox { Accessible.name: qsTr("Vertical scroll steps"); from: -20; to: 20; value: ed.action && ed.action.dy !== undefined ? ed.action.dy : 0; onValueModified: ed.edited(ed.copy({"dy": value})) }
        Label { text: qsTr("Horizontal") }
        SpinBox { Accessible.name: qsTr("Horizontal scroll steps"); from: -20; to: 20; value: ed.action && ed.action.dx !== undefined ? ed.action.dx : 0; onValueModified: ed.edited(ed.copy({"dx": value})) }
    }
    ComboBox {
        visible: ed.action && ed.action.type === "view"
        model: ["top", "bottom", "left", "right", "front", "back", "iso1", "iso2", "fit"]
        currentIndex: Math.max(0, model.indexOf(ed.action ? ed.action.view : "fit"))
        Accessible.name: qsTr("View")
        onActivated: ed.edited(ed.copy({"view": model[currentIndex]}))
    }
    ComboBox {
        visible: ed.action && ed.action.type === "toggle"
        model: ["translation", "rotation", "dominant_axis", "motion"]
        currentIndex: Math.max(0, model.indexOf(ed.action ? ed.action.target : "rotation"))
        Accessible.name: qsTr("Toggle target")
        onActivated: ed.edited(ed.copy({"target": model[currentIndex]}))
    }
    RowLayout {
        visible: ed.action && ed.action.type === "sensitivity"
        Label { text: qsTr("Multiply by") }
        SpinBox {
            Accessible.name: qsTr("Sensitivity factor")
            from: 10; to: 1000; stepSize: 10; editable: true
            value: ed.action && ed.action.factor ? Math.round(ed.action.factor * 100) : 150
            textFromValue: function(v) { return (v / 100).toFixed(2) }
            valueFromText: function(t) { return Math.round(parseFloat(t) * 100) }
            onValueModified: ed.edited(ed.copy({"factor": value / 100}))
        }
    }
    ComboBox {
        visible: ed.action && ed.action.type === "profile_switch"
        Layout.fillWidth: true
        model: ed.profileIds
        currentIndex: Math.max(0, ed.profileIds.indexOf(ed.action ? ed.action.profile : ""))
        Accessible.name: qsTr("Profile")
        onActivated: ed.edited(ed.copy({"profile": ed.profileIds[currentIndex]}))
    }
    TextField {
        visible: ed.action && ed.action.type === "launch"
        Layout.fillWidth: true
        text: ed.action && ed.action.desktop_id ? ed.action.desktop_id : ""
        placeholderText: qsTr("desktop file id, e.g. org.freecad.FreeCAD")
        Accessible.name: qsTr("Application desktop file id")
        onEditingFinished: ed.edited(ed.copy({"desktop_id": text}))
    }
    ColumnLayout {
        visible: ed.action && ed.action.type === "exec"
        Layout.fillWidth: true
        TextField {
            id: argvField
            Layout.fillWidth: true
            text: ed.action && ed.action.argv ? ed.action.argv.join(" ") : ""
            placeholderText: qsTr("program and arguments (no shell)")
            Accessible.name: qsTr("Command")
            onEditingFinished: ed.edited(ed.copy({"argv": text.trim().length ? text.trim().split(/\s+/) : [], "confirmed": false}))
        }
        CheckBox {
            text: qsTr("I wrote this command and want it to run")
            checked: ed.action ? !!ed.action.confirmed : false
            onToggled: ed.edited(ed.copy({"confirmed": checked}))
        }
        Label {
            text: qsTr("Commands run without a shell, only if “Allow command actions” is enabled in Settings. Imported profiles always arrive with commands disabled.")
            wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.7
        }
    }
    // macro: an ordered list of steps (FR-304); max 64 steps, 5 s per delay, 30 s in total
    ColumnLayout {
        id: macroEd
        visible: ed.action && ed.action.type === "macro"
        Layout.fillWidth: true
        readonly property var steps: ed.action && ed.action.type === "macro" && ed.action.steps ? ed.action.steps : []
        readonly property var stepTypes: ["key", "key_down", "key_up", "mouse", "scroll", "delay"]
        readonly property var stepNames: [qsTr("Press keys"), qsTr("Hold keys down"), qsTr("Release keys"), qsTr("Mouse button"), qsTr("Scroll"), qsTr("Wait")]
        readonly property int totalMs: steps.reduce(function(t, st) { return t + (st.type === "delay" ? (st.ms || 0) : 0) }, 0)
        function setSteps(list) { ed.edited(ed.copy({"steps": list})) }
        function stepDefaults(t) {
            switch (t) {
            case "mouse": return {"type": t, "button": "left"};
            case "scroll": return {"type": t, "dx": 0, "dy": 1};
            case "delay": return {"type": t, "ms": 100};
            default: return {"type": t, "keys": "ctrl+c"};
            }
        }
        function patch(i, extra) { var l = JSON.parse(JSON.stringify(steps)); for (var k in extra) l[i][k] = extra[k]; setSteps(l) }
        function move(i, d) { var l = steps.slice(); var t = l[i]; l[i] = l[i + d]; l[i + d] = t; setSteps(l) }
        Label {
            text: qsTr("%1 steps · %2 ms of waits (max 64 steps, 30 s)").arg(macroEd.steps.length).arg(macroEd.totalMs)
            color: macroEd.totalMs > 30000 || macroEd.steps.length > 64 ? "#d93025" : palette.text
            opacity: 0.8
        }
        Repeater {
            model: macroEd.steps
            // two lines per step so the editor fits a narrow window: type and ordering, then parameters
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                RowLayout {
                    Layout.fillWidth: true
                    Label { text: qsTr("%1.").arg(index + 1); Layout.preferredWidth: 24 }
                    ComboBox {
                        Layout.fillWidth: true
                        Layout.maximumWidth: 200
                        model: macroEd.stepNames
                        currentIndex: Math.max(0, macroEd.stepTypes.indexOf(modelData.type))
                        Accessible.name: qsTr("Step %1 type").arg(index + 1)
                        onActivated: { var l = macroEd.steps.slice(); l[index] = macroEd.stepDefaults(macroEd.stepTypes[currentIndex]); macroEd.setSteps(l) }
                    }
                    Item { Layout.fillWidth: true }
                    ToolButton { text: "↑"; enabled: index > 0; Accessible.name: qsTr("Move step %1 up").arg(index + 1); onClicked: macroEd.move(index, -1) }
                    ToolButton { text: "↓"; enabled: index < macroEd.steps.length - 1; Accessible.name: qsTr("Move step %1 down").arg(index + 1); onClicked: macroEd.move(index, 1) }
                    ToolButton { text: "✕"; enabled: macroEd.steps.length > 1; Accessible.name: qsTr("Remove step %1").arg(index + 1); onClicked: { var l = macroEd.steps.slice(); l.splice(index, 1); macroEd.setSteps(l) } }
                }
                RowLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 28
                    TextField {
                        visible: modelData.type === "key" || modelData.type === "key_down" || modelData.type === "key_up"
                        Layout.fillWidth: true
                        text: modelData.keys || ""
                        placeholderText: qsTr("e.g. ctrl+shift+s")
                        Accessible.name: qsTr("Step %1 keys").arg(index + 1)
                        onEditingFinished: if (text !== modelData.keys) macroEd.patch(index, {"keys": text})
                    }
                    ComboBox {
                        visible: modelData.type === "mouse"
                        Layout.fillWidth: true
                        Layout.maximumWidth: 200
                        model: ["left", "right", "middle", "back", "forward"]
                        currentIndex: Math.max(0, model.indexOf(modelData.button || "left"))
                        Accessible.name: qsTr("Step %1 mouse button").arg(index + 1)
                        onActivated: macroEd.patch(index, {"button": model[currentIndex]})
                    }
                    Label { visible: modelData.type === "scroll"; text: qsTr("↕") }
                    SpinBox {
                        visible: modelData.type === "scroll"
                        Layout.preferredWidth: 100
                        from: -20; to: 20; value: modelData.dy || 0
                        Accessible.name: qsTr("Step %1 vertical scroll").arg(index + 1)
                        onValueModified: macroEd.patch(index, {"dy": value})
                    }
                    Label { visible: modelData.type === "scroll"; text: qsTr("↔") }
                    SpinBox {
                        visible: modelData.type === "scroll"
                        Layout.preferredWidth: 100
                        from: -20; to: 20; value: modelData.dx || 0
                        Accessible.name: qsTr("Step %1 horizontal scroll").arg(index + 1)
                        onValueModified: macroEd.patch(index, {"dx": value})
                    }
                    SpinBox {
                        visible: modelData.type === "delay"
                        Layout.preferredWidth: 130
                        from: 1; to: 5000; stepSize: 50; editable: true
                        value: modelData.ms || 100
                        textFromValue: function(v) { return v + " ms" }
                        valueFromText: function(t) { return parseInt(t) || 1 }
                        Accessible.name: qsTr("Step %1 wait in milliseconds").arg(index + 1)
                        onValueModified: macroEd.patch(index, {"ms": value})
                    }
                    Item { Layout.fillWidth: true; visible: modelData.type !== "key" && modelData.type !== "key_down" && modelData.type !== "key_up" }
                }
            }
        }
        RowLayout {
            Button { text: qsTr("Add step"); enabled: macroEd.steps.length < 64; onClicked: { var l = macroEd.steps.slice(); l.push(macroEd.stepDefaults("key")); macroEd.setSteps(l) } }
            Button { text: qsTr("Add wait"); enabled: macroEd.steps.length < 64; onClicked: { var l = macroEd.steps.slice(); l.push(macroEd.stepDefaults("delay")); macroEd.setSteps(l) } }
        }
    }
    // radial menu: up to 8 slices, item 1 at the top, then clockwise
    ColumnLayout {
        visible: ed.action && ed.action.type === "radial"
        Layout.fillWidth: true
        Label { text: qsTr("Slices go clockwise from the top. Hold the button, push or tilt the cap towards a slice, release to run it. Bind it to a button without hold or double-press actions."); wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.7 }
        Repeater {
            model: ed.action && ed.action.type === "radial" && ed.action.items ? ed.action.items : []
            Frame {
                Layout.fillWidth: true
                ColumnLayout {
                    anchors.left: parent.left; anchors.right: parent.right
                    RowLayout {
                        Label { text: qsTr("%1.").arg(index + 1) }
                        TextField {
                            Layout.fillWidth: true
                            text: modelData.label
                            maximumLength: 48
                            Accessible.name: qsTr("Slice %1 label").arg(index + 1)
                            onEditingFinished: if (text !== modelData.label) { var it = ed.action.items.slice(); it[index] = {"label": text, "action": modelData.action}; ed.edited(ed.copy({"items": it})) }
                        }
                        Button {
                            text: qsTr("Remove"); enabled: ed.action.items.length > 2
                            onClicked: { var it = ed.action.items.slice(); it.splice(index, 1); ed.edited(ed.copy({"items": it})) }
                        }
                    }
                    Loader {
                        id: sliceEditor
                        Layout.fillWidth: true
                        // created lazily so the editor doesn't instantiate itself recursively
                        active: ed.allowRadial
                        source: "ActionEditor.qml"
                        onLoaded: { item.allowRadial = false; item.allowNone = false; item.profileIds = Qt.binding(function() { return ed.profileIds }); item.action = modelData.action }
                        Connections {
                            target: sliceEditor.item
                            ignoreUnknownSignals: true
                            function onEdited(a) { var it = ed.action.items.slice(); it[index] = {"label": modelData.label, "action": a}; ed.edited(ed.copy({"items": it})) }
                        }
                    }
                }
            }
        }
        Button {
            text: qsTr("Add slice"); enabled: !!(ed.action && ed.action.items && ed.action.items.length < 8)
            onClicked: { var it = ed.action.items.slice(); it.push({"label": qsTr("Slice %1").arg(it.length + 1), "action": {"type": "none"}}); ed.edited(ed.copy({"items": it})) }
        }
    }
}

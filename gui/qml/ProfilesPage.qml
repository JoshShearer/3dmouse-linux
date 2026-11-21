import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs

// Profile management (FR-401..FR-406).
Item {
    id: page
    property bool active: false
    property var list: []
    property string filter: ""
    // FR-407: search by name, id or output mode
    readonly property var shown: list.filter(function(p) {
        var f = filter.trim().toLowerCase();
        return !f.length || (p.display_name + " " + p.id + " " + p.app_mode).toLowerCase().indexOf(f) >= 0 })
    property var sel: null       // full profile being edited
    property bool dirty: false
    property var importPreview: null
    property string importText: ""

    function reload() {
        agent.call("profiles.list", {}, function(ok, l) { if (ok) { page.list = l; if (!page.sel && win.activeProfile) page.open(win.activeProfile) } })
    }
    function open(id) {
        agent.call("profiles.get", {"profile": id}, function(ok, p) { if (ok) { page.sel = p; page.dirty = false } })
    }
    function edit(fn) { var p = JSON.parse(JSON.stringify(sel)); fn(p); sel = p; dirty = true }
    onActiveChanged: if (active) reload()
    Connections { target: agent; function onAgentEvent(name, body) { if (name === "profile" && page.active) page.reload() } }

    FileDialog {
        id: importDlg
        title: qsTr("Import profile")
        nameFilters: [qsTr("Profiles (*.toml *.json)")]
        onAccepted: {
            page.importText = agent.readFile(selectedFile);
            agent.call("profiles.import", {"text": page.importText, "apply": false}, function(ok, r) {
                if (!ok) { win.toast(r); return }
                page.importPreview = r.preview; importConfirm.open()
            })
        }
    }
    FileDialog {
        id: exportDlg
        title: qsTr("Export profile")
        fileMode: FileDialog.SaveFile
        nameFilters: [qsTr("TOML profile (*.toml)")]
        onAccepted: agent.call("profiles.export", {"profile": page.sel.id}, function(ok, r) {
            win.toast(ok && agent.writeFile(selectedFile, r.toml) ? qsTr("Exported") : qsTr("Export failed"))
        })
    }
    Dialog {
        id: importConfirm
        title: qsTr("Import profile?")
        anchors.centerIn: parent
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        ColumnLayout {
            Label { text: page.importPreview ? qsTr("%1 (%2), %3 button bindings").arg(page.importPreview.display_name).arg(page.importPreview.id).arg(page.importPreview.bindings) : "" }
            Label { visible: page.importPreview && page.importPreview.overwrites_existing; text: qsTr("This replaces your existing profile with the same id."); color: "#b45309" }
            Repeater { model: page.importPreview ? page.importPreview.warnings : []; Label { text: "⚠ " + modelData; wrapMode: Text.Wrap; Layout.maximumWidth: 420 } }
        }
        onAccepted: agent.call("profiles.import", {"text": page.importText, "apply": true, "overwrite": true}, function(ok, r) { win.toast(ok ? qsTr("Imported") : r); page.reload() })
    }
    Dialog {
        id: newDlg
        title: qsTr("New profile")
        anchors.centerIn: parent
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        ColumnLayout {
            Label { text: qsTr("Copies the selected profile.") }
            TextField { id: newName; placeholderText: qsTr("Name"); Layout.preferredWidth: 300; Accessible.name: qsTr("Profile name") }
        }
        onAccepted: {
            var base = page.sel ? JSON.parse(JSON.stringify(page.sel)) : null;
            if (!base) return;
            var id = newName.text.toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "").substring(0, 60) || "custom";
            base.id = id; base.display_name = newName.text || id; base.notes = "";
            agent.call("profiles.save", {"profile": base}, function(ok, r) { win.toast(ok ? qsTr("Created %1").arg(id) : r); page.reload(); if (ok) page.open(id) })
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 12
        ColumnLayout {
            // nested layouts default to fillWidth: true; without this the list competes with
            // the editor for spare width and squeezes it off-screen on wide windows
            Layout.fillWidth: false
            // scales with the page: narrow windows give the editor most of the room
            Layout.preferredWidth: Math.round(Math.max(170, Math.min(300, page.width * 0.28)))
            Layout.maximumWidth: Layout.preferredWidth
            Layout.fillHeight: true
            TextField {
                id: search
                Layout.fillWidth: true
                placeholderText: qsTr("Search profiles")
                Accessible.name: qsTr("Search profiles")
                onTextChanged: page.filter = text
            }
            ListView {
                id: lv
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: page.shown
                keyNavigationEnabled: true
                Accessible.name: qsTr("Profiles")
                delegate: ItemDelegate {
                    width: lv.width
                    highlighted: page.sel && page.sel.id === modelData.id
                    contentItem: ColumnLayout {
                        Label { Layout.fillWidth: true; elide: Text.ElideRight; text: (modelData.active ? "● " : "") + modelData.display_name; font.bold: modelData.active }
                        Label { Layout.fillWidth: true; elide: Text.ElideRight; text: modelData.id + " · " + modelData.app_mode; opacity: 0.6; font.pointSize: 8 }
                    }
                    onClicked: page.open(modelData.id)
                }
            }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Button { text: qsTr("New…"); enabled: page.sel !== null; onClicked: { newName.text = ""; newDlg.open() } }
                Button { text: qsTr("Import…"); onClicked: importDlg.open() }
                Button { text: qsTr("Restore presets"); onClicked: agent.call("profiles.reset_presets", {}, function(ok, r) { win.toast(ok ? qsTr("Presets restored") : r); page.reload() }) }
            }
        }
        ScrollView {
            id: editorScroll
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            Layout.fillHeight: true
            clip: true
            // never scroll sideways: the content reflows to the available width instead
            contentWidth: availableWidth
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
            ScrollBar.vertical.policy: ScrollBar.AlwaysOn
            ColumnLayout {
                // keep clear of the vertical scrollbar
                width: editorScroll.availableWidth - 16
                visible: page.sel !== null
                spacing: 10
                Label { text: page.sel ? page.sel.display_name : ""; font.bold: true; font.pointSize: 13; Layout.fillWidth: true; elide: Text.ElideRight }
                Flow {
                    Layout.fillWidth: true
                    spacing: 6
                    Button { text: qsTr("Use now"); onClicked: agent.call("profiles.activate", {"profile": page.sel.id}, function(ok, r) { win.toast(ok ? qsTr("Active until the next restart or focus rule") : r); page.reload() }) }
                    Button { text: qsTr("Always use"); onClicked: agent.call("profiles.activate", {"profile": page.sel.id, "persist": true}, function(ok, r) { win.toast(ok ? qsTr("Pinned – automatic switching overridden") : r); page.reload() }) }
                    Button { text: qsTr("Export…"); onClicked: exportDlg.open() }
                    Button { text: qsTr("Delete"); enabled: page.sel && page.sel.id !== "default"; onClicked: agent.call("profiles.delete", {"profile": page.sel.id}, function(ok, r) { win.toast(ok ? qsTr("Deleted") : r); page.sel = null; page.reload() }) }
                }
                Label { visible: page.sel && page.sel.notes; text: page.sel ? page.sel.notes : ""; wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.75 }
                GridLayout {
                    // labels above the fields when the editor is narrow
                    columns: editorScroll.availableWidth < 496 ? 1 : 2
                    Layout.fillWidth: true
                    Label { text: qsTr("Name") }
                    TextField { Layout.fillWidth: true; Accessible.name: qsTr("Profile name"); text: page.sel ? page.sel.display_name : ""; onEditingFinished: if (page.sel && text !== page.sel.display_name) page.edit(function(p) { p.display_name = text }) }
                    Label { text: qsTr("Applies to") }
                    ComboBox {
                        Layout.fillWidth: true
                        Accessible.name: qsTr("Applies to")
                        model: [qsTr("Fallback (default)"), qsTr("Specific applications"), qsTr("A specific device")]
                        readonly property var scopes: ["global", "application", "device"]
                        currentIndex: page.sel ? Math.max(0, scopes.indexOf(page.sel.scope)) : 0
                        onActivated: page.edit(function(p) { p.scope = scopes[currentIndex] })
                    }
                    Label { text: qsTr("Application ids") }
                    GridLayout {
                        Layout.fillWidth: true
                        columns: editorScroll.availableWidth < 576 ? 1 : 2
                        TextField {
                            id: appIds
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            Layout.preferredWidth: 100
                            text: page.sel && page.sel.app_selector ? page.sel.app_selector.app_ids.join(", ") : ""
                            placeholderText: qsTr("e.g. blender, org.freecad.FreeCAD")
                            Accessible.name: qsTr("Application ids, comma separated")
                            onEditingFinished: page.edit(function(p) { p.app_selector = p.app_selector || {"app_ids": []}; p.app_selector.app_ids = text.split(",").map(function(s) { return s.trim() }).filter(function(s) { return s.length > 0 }) })
                        }
                        Button {
                            text: qsTr("Add focused app")
                            ToolTip.visible: hovered; ToolTip.text: qsTr("Switch to the application within 3 seconds")
                            onClicked: grabTimer.start()
                            Timer {
                                id: grabTimer; interval: 3000
                                onTriggered: { var a = agent.status.agent.focus.app_id; if (a && a !== "3dmouse-config") page.edit(function(p) { p.app_selector = p.app_selector || {"app_ids": []}; if (p.app_selector.app_ids.indexOf(a) < 0) p.app_selector.app_ids.push(a) }); else win.toast(qsTr("No other application was focused")) }
                            }
                        }
                    }
                    Label { text: qsTr("Window title contains") }
                    TextField {
                        Layout.fillWidth: true
                        Accessible.name: qsTr("Window title contains")
                        text: page.sel && page.sel.app_selector && page.sel.app_selector.title_contains ? page.sel.app_selector.title_contains : ""
                        placeholderText: qsTr("optional – titles are only matched, never stored or logged")
                        onEditingFinished: page.edit(function(p) { p.app_selector = p.app_selector || {"app_ids": []}; p.app_selector.title_contains = text.length ? text : null })
                    }
                    Label { text: qsTr("Window title matches") }
                    TextField {
                        Layout.fillWidth: true
                        Accessible.name: qsTr("Window title regular expression")
                        text: page.sel && page.sel.app_selector && page.sel.app_selector.title_regex ? page.sel.app_selector.title_regex : ""
                        placeholderText: qsTr("optional regular expression, e.g. ^Assembly .* \\| Onshape$")
                        onEditingFinished: page.edit(function(p) { p.app_selector = p.app_selector || {"app_ids": []}; p.app_selector.title_regex = text.length ? text : null })
                    }
                    Label { text: qsTr("Executables") }
                    TextField {
                        Layout.fillWidth: true
                        Accessible.name: qsTr("Executables, comma separated")
                        text: page.sel && page.sel.app_selector && page.sel.app_selector.executables ? page.sel.app_selector.executables.join(", ") : ""
                        placeholderText: qsTr("optional: file name or absolute path, e.g. blender, /opt/cad/bin/cad")
                        onEditingFinished: page.edit(function(p) { p.app_selector = p.app_selector || {"app_ids": []}; p.app_selector.executables = text.split(",").map(function(s) { return s.trim() }).filter(function(s) { return s.length > 0 }) })
                    }
                    Label { text: qsTr("Device") }
                    TextField {
                        Layout.fillWidth: true
                        Accessible.name: qsTr("Device USB id")
                        text: page.sel && page.sel.device_selector ? page.sel.device_selector : ""
                        placeholderText: qsTr("optional USB id, e.g. 256f:c635")
                        onEditingFinished: page.edit(function(p) { p.device_selector = text.length ? text : null })
                    }
                    Label { text: qsTr("Output mode") }
                    ComboBox {
                        Layout.fillWidth: true
                        Accessible.name: qsTr("Output mode")
                        model: [qsTr("Native 6-DOF (libspnav applications)"), qsTr("Application plugin"), qsTr("Desktop emulation (scroll, pointer or keys)"), qsTr("Off – leave the device to the application"), qsTr("Game controller (virtual joystick)")]
                        readonly property var modes: ["native6dof", "application-plugin", "desktop-emulation", "none", "virtual-joystick"]
                        currentIndex: page.sel ? Math.max(0, modes.indexOf(page.sel.native_app_mode)) : 0
                        onActivated: page.edit(function(p) { p.native_app_mode = modes[currentIndex] })
                    }
                }
                Card {
                    id: joyCard
                    title: qsTr("Game controller")
                    visible: page.sel && page.sel.native_app_mode === "virtual-joystick"
                    readonly property bool allowed: agent.status && agent.status.agent && agent.status.agent.capabilities ? !!agent.status.agent.capabilities.virtual_joystick : false
                    Label {
                        Layout.fillWidth: true; wrapMode: Text.Wrap
                        text: qsTr("While this profile is active, the six axes and the buttons appear to games as a controller named “3DMouse Linux virtual joystick”. Native 3D apps and desktop input get nothing.")
                    }
                    Label {
                        Layout.fillWidth: true; wrapMode: Text.Wrap
                        color: "#c5221f"
                        visible: !joyCard.allowed
                        text: qsTr("The virtual joystick is not allowed yet: turn it on in Settings. Until then this profile ignores motion.")
                    }
                }
                Card {
                    id: emuCard
                    title: qsTr("Desktop emulation")
                    visible: page.sel && page.sel.native_app_mode === "desktop-emulation"
                    readonly property var de: page.sel && page.sel.desktop ? page.sel.desktop : ({})
                    readonly property string style: de.style || "scroll"
                    // what + and - mean on each canonical axis (docs/architecture/axis-frames.md)
                    readonly property var axisNames: [qsTr("Push right / left (TX)"), qsTr("Lift / press (TY)"), qsTr("Push away / pull (TZ)"), qsTr("Tilt back / forward (RX)"), qsTr("Twist (RY)"), qsTr("Roll right / left (RZ)")]
                    readonly property var plusNames: [qsTr("right"), qsTr("lift"), qsTr("away"), qsTr("tilt back"), qsTr("anticlockwise"), qsTr("roll right")]
                    readonly property var minusNames: [qsTr("left"), qsTr("press"), qsTr("pull"), qsTr("tilt forward"), qsTr("clockwise"), qsTr("roll left")]
                    function setGestures(fn) { page.edit(function(p) { p.desktop.axis_keys = p.desktop.axis_keys || []; fn(p.desktop.axis_keys) }) }
                    RowLayout {
                        Label { text: qsTr("Motion does") }
                        ComboBox {
                            Layout.fillWidth: true
                            Layout.maximumWidth: 360
                            Accessible.name: qsTr("What motion does")
                            readonly property var styles: ["scroll", "pointer", "keys"]
                            model: [qsTr("Scroll"), qsTr("Move the pointer (accessibility)"), qsTr("Press keys (media, window switching)")]
                            currentIndex: Math.max(0, styles.indexOf(emuCard.style))
                            onActivated: page.edit(function(p) { p.desktop.style = styles[currentIndex] })
                        }
                    }
                    Label {
                        Layout.fillWidth: true; wrapMode: Text.Wrap; opacity: 0.7
                        text: emuCard.style === "pointer" ? qsTr("Push the cap to move the pointer (away = up); lift or press to scroll. Bind buttons to clicks on the Buttons page.")
                              : emuCard.style === "keys" ? qsTr("Push an axis past its threshold to press a key or chord, once or repeatedly while held. Let go (below 60 % of the threshold) to re-arm it.")
                              : qsTr("Push/pull scrolls vertically, left/right scrolls horizontally.")
                    }
                    RowLayout {
                        visible: emuCard.style === "pointer"
                        Label { text: qsTr("Pointer speed") }
                        Slider { Layout.fillWidth: true; Accessible.name: qsTr("Pointer speed"); from: 50; to: 5000; value: emuCard.de.pointer_speed || 1200; onMoved: page.edit(function(p) { p.desktop.pointer_speed = value }) }
                    }
                    RowLayout {
                        visible: emuCard.style !== "keys"
                        Label { text: qsTr("Scroll speed") }
                        Slider { Layout.fillWidth: true; Accessible.name: qsTr("Scroll speed"); from: 200; to: 8000; value: page.sel && page.sel.desktop ? page.sel.desktop.scroll_speed : 2400; onMoved: page.edit(function(p) { p.desktop = p.desktop || {}; p.desktop.scroll_speed = value }) }
                    }
                    Repeater {
                        model: emuCard.style === "keys" && emuCard.de.axis_keys ? emuCard.de.axis_keys : []
                        Frame {
                            Layout.fillWidth: true
                            ColumnLayout {
                                anchors.left: parent.left; anchors.right: parent.right
                                RowLayout {
                                    Layout.fillWidth: true
                                    ComboBox {
                                        Layout.fillWidth: true
                                        Layout.maximumWidth: 260
                                        model: emuCard.axisNames
                                        currentIndex: modelData.axis
                                        Accessible.name: qsTr("Gesture %1 axis").arg(index + 1)
                                        onActivated: { var i = index; var a = currentIndex; emuCard.setGestures(function(l) { l[i].axis = a }) }
                                    }
                                    Item { Layout.fillWidth: true }
                                    ToolButton { text: "✕"; Accessible.name: qsTr("Remove gesture %1").arg(index + 1); onClicked: { var i = index; emuCard.setGestures(function(l) { l.splice(i, 1) }) } }
                                }
                                GridLayout {
                                    Layout.fillWidth: true
                                    columns: 2
                                    Label { text: qsTr("%1 →").arg(emuCard.plusNames[modelData.axis] || "+") }
                                    TextField {
                                        Layout.fillWidth: true
                                        text: modelData.positive || ""
                                        placeholderText: qsTr("key or chord, e.g. volumedown")
                                        Accessible.name: qsTr("Gesture %1 key for %2").arg(index + 1).arg(emuCard.plusNames[modelData.axis] || "+")
                                        onEditingFinished: { var i = index; var t = text; if (t !== (modelData.positive || "")) emuCard.setGestures(function(l) { l[i].positive = t }) }
                                    }
                                    Label { text: qsTr("%1 →").arg(emuCard.minusNames[modelData.axis] || "−") }
                                    TextField {
                                        Layout.fillWidth: true
                                        text: modelData.negative || ""
                                        placeholderText: qsTr("key or chord, e.g. volumeup")
                                        Accessible.name: qsTr("Gesture %1 key for %2").arg(index + 1).arg(emuCard.minusNames[modelData.axis] || "−")
                                        onEditingFinished: { var i = index; var t = text; if (t !== (modelData.negative || "")) emuCard.setGestures(function(l) { l[i].negative = t }) }
                                    }
                                }
                                Flow {
                                    Layout.fillWidth: true
                                    spacing: 8
                                    Label { text: qsTr("Threshold"); height: thr.height; verticalAlignment: Text.AlignVCenter }
                                    SpinBox {
                                        id: thr
                                        from: 20; to: 95; stepSize: 5
                                        value: Math.round((modelData.threshold || 0.5) * 100)
                                        textFromValue: function(v) { return v + " %" }
                                        valueFromText: function(t) { return parseInt(t) || 50 }
                                        Accessible.name: qsTr("Gesture %1 threshold").arg(index + 1)
                                        onValueModified: { var i = index; var v = value / 100; emuCard.setGestures(function(l) { l[i].threshold = v }) }
                                    }
                                    Label { text: qsTr("Repeat"); height: thr.height; verticalAlignment: Text.AlignVCenter }
                                    SpinBox {
                                        from: 0; to: 2000; stepSize: 50
                                        value: modelData.repeat_ms || 0
                                        textFromValue: function(v) { return v === 0 ? qsTr("once") : qsTr("every %1 ms").arg(v) }
                                        valueFromText: function(t) { return parseInt(t) || 0 }
                                        Accessible.name: qsTr("Gesture %1 repeat interval").arg(index + 1)
                                        // 1..99 ms is not allowed: step over it in whichever direction the user went
                                        onValueModified: { var i = index; var v = value > 0 && value < 100 ? ((modelData.repeat_ms || 0) >= 100 ? 0 : 100) : value; emuCard.setGestures(function(l) { l[i].repeat_ms = v }) }
                                    }
                                }
                            }
                        }
                    }
                    Button {
                        visible: emuCard.style === "keys"
                        text: qsTr("Add gesture")
                        enabled: !emuCard.de.axis_keys || emuCard.de.axis_keys.length < 12
                        onClicked: emuCard.setGestures(function(l) { l.push({"axis": 0, "positive": "", "negative": "", "threshold": 0.5, "repeat_ms": 0}) })
                    }
                    RowLayout {
                        visible: emuCard.style !== "keys"
                        CheckBox { text: qsTr("Invert vertical"); checked: page.sel && page.sel.desktop ? page.sel.desktop.invert_vertical : false; onToggled: page.edit(function(p) { p.desktop.invert_vertical = checked }) }
                        CheckBox { text: qsTr("Invert horizontal"); checked: page.sel && page.sel.desktop ? page.sel.desktop.invert_horizontal : false; onToggled: page.edit(function(p) { p.desktop.invert_horizontal = checked }) }
                    }
                }
                Card {
                    title: qsTr("Axis directions")
                    visible: page.sel !== null
                    Label {
                        Layout.fillWidth: true; wrapMode: Text.Wrap; opacity: 0.7
                        text: qsTr("Reverse any axis for this profile only. Applies to native apps (Blender, FreeCAD…) and desktop scrolling. Speed, dead zone and axis remapping are on the Navigation page.")
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 4
                        Repeater {
                            model: [qsTr("Pan left/right (TX)"), qsTr("Up/down (TY)"), qsTr("Push/pull (TZ)"), qsTr("Tilt fwd/back (RX)"), qsTr("Twist (RY)"), qsTr("Tilt left/right (RZ)")]
                            delegate: CheckBox {
                                required property int index
                                required property string modelData
                                text: qsTr("Invert %1").arg(modelData)
                                checked: page.sel && page.sel.navigation ? page.sel.navigation.per_axis_invert[index] : false
                                Accessible.name: text
                                onToggled: { var i = index; var c = checked; page.edit(function(p) { p.navigation.per_axis_invert[i] = c }) }
                            }
                        }
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 6
                        Button { text: qsTr("Invert all"); onClicked: page.edit(function(p) { p.navigation.per_axis_invert = [true, true, true, true, true, true] }) }
                        Button { text: qsTr("Invert rotations"); onClicked: page.edit(function(p) { for (var i = 3; i < 6; ++i) p.navigation.per_axis_invert[i] = !p.navigation.per_axis_invert[i] }) }
                        Button { text: qsTr("Invert pan/zoom"); onClicked: page.edit(function(p) { for (var i = 0; i < 3; ++i) p.navigation.per_axis_invert[i] = !p.navigation.per_axis_invert[i] }) }
                        Button { text: qsTr("Clear"); onClicked: page.edit(function(p) { p.navigation.per_axis_invert = [false, false, false, false, false, false] }) }
                    }
                }
                RowLayout {
                    Item { Layout.fillWidth: true }
                    Button { text: qsTr("Revert"); enabled: page.dirty; onClicked: page.open(page.sel.id) }
                    Button { text: qsTr("Save"); enabled: page.dirty; highlighted: true; onClicked: agent.call("profiles.save", {"profile": page.sel}, function(ok, r) { win.toast(ok ? qsTr("Saved") : r); if (ok) { page.dirty = false; page.reload() } }) }
                }
            }
        }
    }
}

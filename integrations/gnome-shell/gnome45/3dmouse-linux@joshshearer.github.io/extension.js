// 3DMouse Linux focus bridge (GNOME Shell 45+, ESM).
// GNOME Wayland exposes no focus API to ordinary clients, so this extension
// pushes `focus.report` to $XDG_RUNTIME_DIR/3dmouse/agent.sock.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

function appIdOf(win) {
    if (!win)
        return '';
    return win.get_sandboxed_app_id?.() || win.get_gtk_application_id?.() || win.get_wm_class() || '';
}

export default class ThreeDMouseFocusBridge extends Extension {
    enable() {
        this._path = GLib.build_filenamev([GLib.get_user_runtime_dir(), '3dmouse', 'agent.sock']);
        this._last = null;
        this._focusId = global.display.connect('notify::focus-window', () => this._report());
        this._report();
    }

    disable() {
        if (this._focusId)
            global.display.disconnect(this._focusId);
        this._focusId = 0;
        this._last = null;
    }

    _report() {
        const win = global.display.focus_window;
        const msg = {v: 1, id: 1, cmd: 'focus.report', app_id: appIdOf(win), title: win?.get_title() ?? '', pid: win?.get_pid() ?? 0};
        const key = `${msg.app_id}\n${msg.title}`;
        if (key === this._last || !msg.app_id)
            return;
        this._last = key;
        const client = new Gio.SocketClient();
        client.connect_async(Gio.UnixSocketAddress.new(this._path), null, (c, res) => {
            try {
                const conn = c.connect_finish(res);
                const bytes = new TextEncoder().encode(JSON.stringify(msg) + '\n');
                conn.get_output_stream().write_all_async(bytes, GLib.PRIORITY_DEFAULT, null, (s, r) => {
                    try { s.write_all_finish(r); } catch (e) { /* agent went away */ }
                    conn.close_async(GLib.PRIORITY_DEFAULT, null, null);
                });
            } catch (e) {
                // agent not running: nothing to do; retry on next focus change
                this._last = null;
            }
        });
    }
}

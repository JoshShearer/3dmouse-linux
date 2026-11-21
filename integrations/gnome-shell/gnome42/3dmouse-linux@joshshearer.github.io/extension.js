// 3DMouse Linux focus bridge (GNOME Shell 42-44, legacy imports).
/* exported init */
const {Gio, GLib} = imports.gi;

function appIdOf(win) {
    if (!win)
        return '';
    return (win.get_sandboxed_app_id && win.get_sandboxed_app_id()) || (win.get_gtk_application_id && win.get_gtk_application_id()) || win.get_wm_class() || '';
}

class ThreeDMouseFocusBridge {
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
        const msg = {v: 1, id: 1, cmd: 'focus.report', app_id: appIdOf(win), title: win ? win.get_title() || '' : '', pid: win ? win.get_pid() : 0};
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
                    try { s.write_all_finish(r); } catch (e) { }
                    conn.close_async(GLib.PRIORITY_DEFAULT, null, null);
                });
            } catch (e) {
                this._last = null;
            }
        });
    }
}

function init() {
    return new ThreeDMouseFocusBridge();
}

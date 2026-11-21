// 3dmouse-osd – on-screen overlay for 3DMouse Linux (FR-308, FR-504).
//
// Draws OSD messages, the button help and radial menus that 3dmouse-agent
// announces as events. Where the compositor supports wlr-layer-shell
// (Hyprland, Sway, KDE, COSMIC…) the windows are overlay-layer surfaces that
// never take focus or input. On X11 they are frameless, always-on-top,
// input-transparent windows. GNOME Wayland has no layer-shell: the overlay
// exits and the agent keeps using desktop notifications (honest capability
// reporting, SRS FR-504).
#include "agentclient.h"

#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickWindow>
#include <QScreen>
#include <cstdio>
#ifdef SM_HAVE_LAYERSHELL
#include <LayerShellQt/Window>
#endif

// Places overlay windows; called from QML whenever a window is shown.
class OsdPlacer : public QObject
{
    Q_OBJECT
public:
    bool layerShell = false;
    Q_INVOKABLE void place(QWindow *w, const QString &position, int margin)
    {
        if (!w)
            return;
#ifdef SM_HAVE_LAYERSHELL
        if (layerShell) {
            auto *ls = LayerShellQt::Window::get(w);
            if (position == QLatin1String("top"))
                ls->setAnchors(LayerShellQt::Window::AnchorTop);
            else if (position == QLatin1String("bottom"))
                ls->setAnchors(LayerShellQt::Window::AnchorBottom);
            else
                ls->setAnchors({}); // centred by the compositor
            ls->setMargins(QMargins(margin, margin, margin, margin));
            return;
        }
#endif
        const QRect g = (w->screen() ? w->screen() : QGuiApplication::primaryScreen())->availableGeometry();
        int y = g.center().y() - w->height() / 2;
        if (position == QLatin1String("top"))
            y = g.top() + margin;
        else if (position == QLatin1String("bottom"))
            y = g.bottom() - margin - w->height();
        w->setPosition(g.center().x() - w->width() / 2, y);
    }
};

int main(int argc, char *argv[])
{
    const QByteArray desktop = qgetenv("XDG_CURRENT_DESKTOP").toUpper();
    const bool wayland = !qEnvironmentVariableIsEmpty("WAYLAND_DISPLAY");
    if (wayland && desktop.contains("GNOME") && qEnvironmentVariableIsEmpty("SM_OSD_FORCE")) {
        std::fprintf(stderr, "3dmouse-osd: GNOME has no layer-shell; the agent uses notifications instead\n");
        return 0;
    }
#ifndef SM_HAVE_LAYERSHELL
    if (wayland && qEnvironmentVariableIsEmpty("SM_OSD_FORCE")) {
        std::fprintf(stderr, "3dmouse-osd: built without LayerShellQt; overlays need X11 here. Notifications stay in use.\n");
        return 0;
    }
#endif
    QGuiApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("3dmouse-osd"));
    QGuiApplication::setDesktopFileName(QStringLiteral("io.github.joshshearer._3dmouse_linux.osd"));
    QGuiApplication::setQuitOnLastWindowClosed(false);

    AgentClient agent;
    OsdPlacer placer;
    placer.layerShell = QGuiApplication::platformName().startsWith(QLatin1String("wayland"));
    QQmlApplicationEngine engine;
    agent.setEngine(&engine);
    QObject::connect(&app, &QGuiApplication::aboutToQuit, &agent, [&agent] { agent.shutdown(); });
    engine.rootContext()->setContextProperty(QStringLiteral("agent"), &agent);
    engine.rootContext()->setContextProperty(QStringLiteral("placer"), &placer);
    engine.load(QUrl(QStringLiteral("qrc:/qml/Osd.qml")));
    if (engine.rootObjects().isEmpty()) {
        // QML failed to load (e.g. missing qml6-module-* packages): detach the agent
        // client from the engine first, or its destructor touches freed JS state
        agent.shutdown();
        return 1;
    }

    // Configure every overlay window before it is first shown.
    for (QWindow *w : engine.rootObjects().first()->findChildren<QWindow *>()) {
        w->setFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint | Qt::Tool | Qt::WindowTransparentForInput | Qt::WindowDoesNotAcceptFocus);
#ifdef SM_HAVE_LAYERSHELL
        if (placer.layerShell) {
            auto *ls = LayerShellQt::Window::get(w);
            ls->setLayer(LayerShellQt::Window::LayerOverlay);
            ls->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
            ls->setExclusiveZone(-1);
            ls->setScope(QStringLiteral("3dmouse-osd"));
        }
#endif
    }
    std::fprintf(stderr, "3dmouse-osd: %s overlay\n", placer.layerShell ? "layer-shell" : "X11");
    return app.exec();
}

#include "osd_main.moc"

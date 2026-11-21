// 3dmouse-config – 3DMouse Linux settings application.
#include "agentclient.h"

#include <QGuiApplication>
#include <QIcon>
#include <memory>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QTimer>
#include <QDir>

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("3dmouse-config"));
    QGuiApplication::setApplicationDisplayName(QStringLiteral("3DMouse Settings"));
    QGuiApplication::setApplicationVersion(QStringLiteral(SM_VERSION));
    QGuiApplication::setDesktopFileName(QStringLiteral("io.github.joshshearer._3dmouse_linux.config"));
    QGuiApplication::setWindowIcon(QIcon::fromTheme(QStringLiteral("3dmouse-linux")));
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE"))
        QQuickStyle::setStyle(QStringLiteral("Fusion"));

    AgentClient agent;
    QQmlApplicationEngine engine;
    agent.setEngine(&engine);
    QObject::connect(&app, &QGuiApplication::aboutToQuit, &agent, [&agent] { agent.shutdown(); });
    engine.rootContext()->setContextProperty(QStringLiteral("agent"), &agent);
    const QString shots = qEnvironmentVariable("SM_GUI_SCREENSHOTS");
    engine.rootContext()->setContextProperty(QStringLiteral("screenshotMode"), !shots.isEmpty());
    engine.load(QUrl(QStringLiteral("qrc:/qml/Main.qml")));
    if (engine.rootObjects().isEmpty()) {
        // QML failed to load (e.g. missing qml6-module-* packages): detach the agent
        // client from the engine first, or its destructor touches freed JS state
        agent.shutdown();
        return 1;
    }

    // Development aid: SM_GUI_SCREENSHOTS=<dir> renders every page to PNG
    // and exits (used for docs; run with QT_QPA_PLATFORM=offscreen).
    if (!shots.isEmpty()) {
        auto *win = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        QObject *nav = win ? win->findChild<QObject *>(QStringLiteral("nav")) : nullptr;
        if (!win || !nav)
            return 1;
        // SM_GUI_SCREENSHOT_SIZE=WxH renders at another size, e.g. a wide maximised window
        const QStringList sz = qEnvironmentVariable("SM_GUI_SCREENSHOT_SIZE").split(QLatin1Char('x'));
        if (sz.size() == 2 && sz[0].toInt() > 0 && sz[1].toInt() > 0)
            win->resize(sz[0].toInt(), sz[1].toInt());
        QDir().mkpath(shots);
        auto *t = new QTimer(&app);
        auto page = std::make_shared<int>(-1);
        QObject::connect(t, &QTimer::timeout, &app, [=] {
            if (*page >= 0) {
                const QString f = QStringLiteral("%1/page-%2.png").arg(shots).arg(*page);
                const QImage img = win->grabWindow();
                if (img.isNull() || !img.save(f))
                    qWarning("screenshot of page %d failed (window not exposed?)", *page);
            }
            // pages 0-6 are the sections; 7 is the first-run wizard
            if (++*page >= 8) {
                QGuiApplication::quit();
                return;
            }
            if (*page < 7)
                nav->setProperty("currentIndex", *page);
            else if (QObject *w = win->findChild<QObject *>(QStringLiteral("setupWizard")))
                QMetaObject::invokeMethod(w, "open");
        });
        t->start(1500);
    }
    return app.exec();
}

// 3dmouse-tray – optional status indicator for 3DMouse Linux (FR-807).
//
// A StatusNotifierItem (through QSystemTrayIcon, which Qt exports over D-Bus
// where a tray host exists: waybar, KDE, COSMIC, GNOME with the AppIndicator
// extension, XFCE…). Shows device and profile state and switches profiles.
// Opt-in: the 3dmouse-tray user service is off until the user enables it in
// Settings. Where no tray host appears the indicator exits quietly, so
// enabling it never puts a prompt on screen (FR-807 "no desktop prompt").
#include "agentclient.h"

#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QIcon>
#include <QMenu>
#include <QPainter>
#include <QProcess>
#include <QSystemTrayIcon>
#include <QTimer>
#include <cstdio>
#include <memory>

namespace {

QIcon baseIcon()
{
    QIcon i = QIcon::fromTheme(QStringLiteral("3dmouse-linux"));
    if (i.isNull())
        i = QIcon(QStringLiteral(":/assets/3dmouse-linux.svg"));
    return i;
}

// Greyed out while there is no device or the agent is unreachable, so the
// state is visible without opening the menu.
QIcon stateIcon(bool ok)
{
    const QIcon b = baseIcon();
    if (ok)
        return b;
    QIcon out;
    for (int s : {16, 22, 24, 32, 48, 64})
        out.addPixmap(b.pixmap(s, s, QIcon::Disabled));
    return out;
}

class Tray : public QObject
{
public:
    Tray()
    {
        m_menu.addAction(&m_state);
        m_menu.addAction(&m_profile);
        m_state.setEnabled(false);
        m_profile.setEnabled(false);
        m_menu.addSeparator();
        m_switch = m_menu.addMenu(tr("Switch profile"));
        connect(m_switch, &QMenu::aboutToShow, this, &Tray::fillProfiles);
        m_menu.addAction(tr("Open 3DMouse Settings"), this, &Tray::openSettings);
        m_menu.addSeparator();
        m_menu.addAction(tr("Hide status icon"), this, [] {
            // the service is the opt-in switch; turning it off also stops this process
            QProcess::startDetached(QStringLiteral("systemctl"),
                                    {QStringLiteral("--user"), QStringLiteral("disable"), QStringLiteral("--now"),
                                     QStringLiteral("3dmouse-tray.service")});
            QTimer::singleShot(1500, qApp, &QApplication::quit);
        });
        m_icon.setContextMenu(&m_menu);
        connect(&m_icon, &QSystemTrayIcon::activated, this, [this](QSystemTrayIcon::ActivationReason r) {
            if (r == QSystemTrayIcon::Trigger || r == QSystemTrayIcon::DoubleClick)
                openSettings();
        });
        connect(&m_agent, &AgentClient::statusChanged, this, &Tray::update);
        connect(&m_agent, &AgentClient::connectedChanged, this, &Tray::update);
        update();
        m_icon.show();
    }
    ~Tray() override { m_agent.shutdown(); }

private:
    void update()
    {
        const QVariantMap st = m_agent.status();
        const QVariantMap agent = st.value(QStringLiteral("agent")).toMap();
        const QVariantMap profile = st.value(QStringLiteral("profile")).toMap();
        const QVariantList devices = st.value(QStringLiteral("broker")).toMap().value(QStringLiteral("devices")).toList();
        const QString devName = devices.isEmpty() ? QString() : devices.first().toMap().value(QStringLiteral("name")).toString();
        QString state;
        bool ok = false;
        if (!m_agent.connected())
            state = tr("Agent not running");
        else if (!agent.value(QStringLiteral("broker_connected")).toBool())
            state = tr("System service not running");
        else if (devName.isEmpty())
            state = tr("No device connected");
        else {
            ok = true;
            state = agent.value(QStringLiteral("locked")).toBool() ? tr("%1 · paused (screen locked)").arg(devName) : devName;
        }
        const QString name = profile.value(QStringLiteral("display_name")).toString();
        const bool manual = profile.value(QStringLiteral("selected_by")).toString() == QLatin1String("manual");
        const QString prof = name.isEmpty() ? QString()
                                            : (manual ? tr("Profile: %1 (manual)") : tr("Profile: %1")).arg(name);
        m_state.setText(state);
        m_profile.setText(prof);
        m_profile.setVisible(!prof.isEmpty());
        m_switch->setEnabled(m_agent.connected());
        m_icon.setToolTip(prof.isEmpty() ? tr("3D mouse: %1").arg(state) : tr("3D mouse: %1\n%2").arg(state, prof));
        if (ok != m_ok || m_first) {
            m_icon.setIcon(stateIcon(ok));
            m_ok = ok;
            m_first = false;
        }
    }

    void fillProfiles()
    {
        m_switch->clear();
        auto *loading = m_switch->addAction(tr("Loading…"));
        loading->setEnabled(false);
        const QVariantMap profile = m_agent.status().value(QStringLiteral("profile")).toMap();
        const QString active = profile.value(QStringLiteral("id")).toString();
        const bool manual = profile.value(QStringLiteral("selected_by")).toString() == QLatin1String("manual");
        m_agent.callNative(QStringLiteral("profiles.list"), {}, [this, active, manual](bool ok, const QVariant &r) {
            m_switch->clear();
            if (!ok) {
                m_switch->addAction(r.toString())->setEnabled(false);
                return;
            }
            auto *group = new QActionGroup(m_switch);
            auto *autoSel = m_switch->addAction(tr("Automatic (follow the focused app)"));
            autoSel->setCheckable(true);
            autoSel->setChecked(!manual);
            group->addAction(autoSel);
            connect(autoSel, &QAction::triggered, this, [this] { activate(QVariant()); });
            m_switch->addSeparator();
            for (const QVariant &v : r.toList()) {
                const QVariantMap p = v.toMap();
                const QString id = p.value(QStringLiteral("id")).toString();
                auto *a = m_switch->addAction(p.value(QStringLiteral("display_name")).toString());
                a->setCheckable(true);
                a->setChecked(manual && id == active);
                group->addAction(a);
                connect(a, &QAction::triggered, this, [this, id] { activate(id); });
            }
        });
    }

    // a session override, like the GUI's profile switch; null returns to automatic selection
    void activate(const QVariant &id)
    {
        m_agent.callNative(QStringLiteral("profiles.activate"), {{QStringLiteral("profile"), id}},
                           [this](bool ok, const QVariant &r) {
                               if (!ok)
                                   m_icon.showMessage(tr("3DMouse Linux"), r.toString(), QSystemTrayIcon::Warning, 4000);
                               m_agent.refresh();
                           });
    }

    void openSettings() { QProcess::startDetached(QStringLiteral("3dmouse-config"), {}); }

    AgentClient m_agent;
    QSystemTrayIcon m_icon;
    QMenu m_menu;
    QMenu *m_switch = nullptr;
    QAction m_state;
    QAction m_profile;
    bool m_ok = false;
    bool m_first = true;
};

} // namespace

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);
    QApplication::setApplicationName(QStringLiteral("3dmouse-tray"));
    QApplication::setApplicationVersion(QStringLiteral(SM_VERSION));
    QApplication::setDesktopFileName(QStringLiteral("io.github.joshshearer._3dmouse_linux.config"));
    QApplication::setQuitOnLastWindowClosed(false);
    if (app.arguments().contains(QStringLiteral("--version"))) {
        std::printf("3dmouse-tray %s\n", SM_VERSION);
        return 0;
    }

    // The tray host (a panel such as waybar) may start after us at login: wait for it
    // for a while, then give up quietly instead of showing anything.
    int waited = 0;
    std::unique_ptr<Tray> tray;
    QTimer probe;
    probe.setInterval(1000);
    QObject::connect(&probe, &QTimer::timeout, &app, [&] {
        if (QSystemTrayIcon::isSystemTrayAvailable()) {
            probe.stop();
            tray = std::make_unique<Tray>();
        } else if (++waited >= 60) {
            std::fprintf(stderr, "3dmouse-tray: no system tray (StatusNotifierItem host) found; exiting\n");
            QApplication::exit(0);
        }
    });
    if (QSystemTrayIcon::isSystemTrayAvailable())
        tray = std::make_unique<Tray>();
    else
        probe.start();
    const int rc = app.exec();
    tray.reset();
    return rc;
}

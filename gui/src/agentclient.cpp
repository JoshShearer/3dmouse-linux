#include "agentclient.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QStandardPaths>
#include <QUrl>
#include <unistd.h>
#include <utility>

static QString defaultSocket()
{
    const QByteArray env = qgetenv("THREEDMOUSE_AGENT_SOCKET");
    if (!env.isEmpty())
        return QString::fromLocal8Bit(env);
    QString rt = QString::fromLocal8Bit(qgetenv("XDG_RUNTIME_DIR"));
    if (rt.isEmpty())
        rt = QStringLiteral("/run/user/%1").arg(getuid());
    return rt + QStringLiteral("/3dmouse/agent.sock");
}

AgentClient::AgentClient(QObject *parent) : QObject(parent), m_path(defaultSocket())
{
    for (int i = 0; i < 6; ++i) {
        m_raw << 0;
        m_norm << 0.0;
        m_spnav << 0;
    }
    connect(&m_sock, &QLocalSocket::readyRead, this, &AgentClient::onReadyRead);
    connect(&m_sock, &QLocalSocket::connected, this, [this] {
        m_error.clear();
        emit connectedChanged();
        subscribe();
        refresh();
    });
    connect(&m_sock, &QLocalSocket::disconnected, this, [this] {
        m_error = tr("Connection to the 3DMouse Linux agent was lost.");
        // fail pending callbacks so the UI never waits forever
        const auto cbs = m_callbacks;
        m_callbacks.clear();
        for (auto cb : cbs)
            if (cb.isCallable())
                cb.call({false, m_error});
        const auto natives = std::exchange(m_native, {});
        for (const auto &r : natives)
            r(false, m_error);
        emit connectedChanged();
        m_retry.start();
    });
    connect(&m_sock, &QLocalSocket::errorOccurred, this, [this](QLocalSocket::LocalSocketError) {
        m_error = tr("3DMouse Linux agent not reachable at %1 (%2). Start it with: systemctl --user enable --now 3dmouse-agent")
                      .arg(m_path, m_sock.errorString());
        emit connectedChanged();
        if (!m_retry.isActive())
            m_retry.start();
    });
    m_retry.setInterval(2000);
    m_retry.setSingleShot(true);
    connect(&m_retry, &QTimer::timeout, this, &AgentClient::tryConnect);
    m_poll.setInterval(2000);
    connect(&m_poll, &QTimer::timeout, this, &AgentClient::refresh);
    m_poll.start();
    tryConnect();
}

void AgentClient::tryConnect()
{
    if (m_sock.state() != QLocalSocket::UnconnectedState)
        return;
    m_sock.connectToServer(m_path);
}

void AgentClient::subscribe()
{
    call(QStringLiteral("subscribe"), {{QStringLiteral("live"), m_live}, {QStringLiteral("events"), true}});
}

void AgentClient::setLive(bool on)
{
    if (on == m_live)
        return;
    m_live = on;
    if (connected())
        subscribe();
    emit liveChanged();
}

void AgentClient::refresh()
{
    if (!connected())
        return;
    call(QStringLiteral("status"));
}

quint64 AgentClient::send(const QString &cmd, const QVariantMap &args)
{
    QJsonObject o = QJsonObject::fromVariantMap(args);
    const quint64 id = m_nextId++;
    o.insert(QStringLiteral("v"), 1);
    o.insert(QStringLiteral("id"), qint64(id));
    o.insert(QStringLiteral("cmd"), cmd);
    m_sock.write(QJsonDocument(o).toJson(QJsonDocument::Compact) + '\n');
    return id;
}

void AgentClient::call(const QString &cmd, const QVariantMap &args, const QJSValue &callback)
{
    if (!connected()) {
        if (callback.isCallable())
            QJSValue(callback).call({false, m_error.isEmpty() ? tr("not connected") : m_error});
        return;
    }
    const quint64 id = send(cmd, args);
    if (callback.isCallable() || cmd == QLatin1String("status"))
        m_callbacks.insert(id, callback);
}

void AgentClient::callNative(const QString &cmd, const QVariantMap &args, Reply reply)
{
    if (!connected()) {
        if (reply)
            reply(false, m_error.isEmpty() ? tr("not connected") : m_error);
        return;
    }
    const quint64 id = send(cmd, args);
    if (reply)
        m_native.insert(id, std::move(reply));
}

void AgentClient::onReadyRead()
{
    m_buf += m_sock.readAll();
    if (m_buf.size() > (4 << 20)) { // misbehaving peer
        m_buf.clear();
        m_sock.abort();
        return;
    }
    int nl;
    while ((nl = m_buf.indexOf('\n')) >= 0) {
        const QByteArray line = m_buf.left(nl);
        m_buf.remove(0, nl + 1);
        const QJsonDocument d = QJsonDocument::fromJson(line);
        if (d.isObject())
            handle(d.object());
    }
}

static QVariantList toList(const QJsonValue &v)
{
    QVariantList out;
    for (const auto &x : v.toArray())
        out << x.toVariant();
    return out;
}

void AgentClient::handle(const QJsonObject &o)
{
    const QString ev = o.value(QStringLiteral("event")).toString();
    if (!ev.isEmpty()) {
        if (ev == QLatin1String("motion")) {
            m_raw = toList(o.value(QStringLiteral("raw")));
            m_norm = toList(o.value(QStringLiteral("norm")));
            m_spnav = toList(o.value(QStringLiteral("spnav")));
            emit motion();
        } else if (ev == QLatin1String("button")) {
            const int b = o.value(QStringLiteral("button")).toInt();
            if (b >= 0 && b < 64) {
                const qulonglong bit = 1ull << b;
                if (o.value(QStringLiteral("pressed")).toBool())
                    m_buttons |= bit;
                else
                    m_buttons &= ~bit;
                emit buttonsChanged();
            }
        } else if (ev == QLatin1String("profile") || ev == QLatin1String("device") || ev == QLatin1String("lock")
                   || ev == QLatin1String("broker") || ev == QLatin1String("focus")) {
            refresh();
        }
        emit agentEvent(ev, o.toVariantMap());
        return;
    }
    const quint64 id = quint64(o.value(QStringLiteral("id")).toVariant().toULongLong());
    const bool ok = o.value(QStringLiteral("ok")).toBool();
    QJSValue cb = m_callbacks.take(id);
    const QVariant result = ok ? o.value(QStringLiteral("result")).toVariant()
                               : QVariant(o.value(QStringLiteral("error")).toObject().value(QStringLiteral("message")).toString());
    if (ok && result.typeId() == QMetaType::QVariantMap && result.toMap().contains(QStringLiteral("agent"))) {
        m_status = result.toMap();
        emit statusChanged();
    }
    if (cb.isCallable())
        cb.call({ok, m_engine ? m_engine->toScriptValue(result) : QJSValue()});
    if (Reply r = m_native.take(id))
        r(ok, result);
}

void AgentClient::runCtl(const QStringList &args, const QJSValue &callback)
{
    runProcess(QStringLiteral("3dmousectl"), args, callback);
}

void AgentClient::userUnit(const QString &verb, const QString &unit, const QJSValue &callback)
{
    static const QStringList units{QStringLiteral("3dmouse-tray.service"), QStringLiteral("3dmouse-osd.service")};
    QStringList args{QStringLiteral("--user")};
    if (verb == QLatin1String("is-enabled"))
        args << verb;
    else if (verb == QLatin1String("enable") || verb == QLatin1String("disable"))
        args << verb << QStringLiteral("--now");
    if (args.size() < 2 || !units.contains(unit)) {
        if (callback.isCallable())
            QJSValue(callback).call({false, QStringLiteral("unsupported unit request")});
        return;
    }
    runProcess(QStringLiteral("systemctl"), args << unit, callback);
}

void AgentClient::runProcess(const QString &program, const QStringList &args, const QJSValue &callback)
{
    auto *p = new QProcess(this);
    QJSValue cb = callback;
    connect(p, &QProcess::finished, this, [p, cb](int code, QProcess::ExitStatus) mutable {
        const QString out = QString::fromUtf8(p->readAllStandardOutput()) + QString::fromUtf8(p->readAllStandardError());
        if (cb.isCallable())
            cb.call({code == 0, out});
        p->deleteLater();
    });
    connect(p, &QProcess::errorOccurred, this, [p, cb, program](QProcess::ProcessError) mutable {
        if (cb.isCallable())
            cb.call({false, QStringLiteral("cannot run %1: %2").arg(program, p->errorString())});
        p->deleteLater();
    });
    p->start(program, args);
}

QString AgentClient::toJson(const QVariant &v, bool pretty) const
{
    return QString::fromUtf8(QJsonDocument::fromVariant(v).toJson(pretty ? QJsonDocument::Indented : QJsonDocument::Compact));
}

QVariant AgentClient::fromJson(const QString &s) const
{
    return QJsonDocument::fromJson(s.toUtf8()).toVariant();
}

QString AgentClient::readFile(const QUrl &url) const
{
    QFile f(url.isLocalFile() ? url.toLocalFile() : url.toString());
    if (!f.open(QIODevice::ReadOnly) || f.size() > (1 << 20))
        return {};
    return QString::fromUtf8(f.readAll());
}

bool AgentClient::writeFile(const QUrl &url, const QString &text) const
{
    QFile f(url.isLocalFile() ? url.toLocalFile() : url.toString());
    if (!f.open(QIODevice::WriteOnly | QIODevice::Truncate))
        return false;
    return f.write(text.toUtf8()) >= 0;
}

// AgentClient – JSON-lines client for the 3dmouse-agent session socket.
// The GUI never talks to devices or the broker directly (SRS §4.1).
#pragma once

#include <QHash>
#include <QJSEngine>
#include <QJSValue>
#include <QLocalSocket>
#include <QObject>
#include <QTimer>
#include <QVariant>
#include <functional>

class AgentClient : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool connected READ connected NOTIFY connectedChanged)
    Q_PROPERTY(QString error READ error NOTIFY connectedChanged)
    Q_PROPERTY(QVariantMap status READ status NOTIFY statusChanged)
    Q_PROPERTY(bool live READ live WRITE setLive NOTIFY liveChanged)
    Q_PROPERTY(QVariantList raw READ raw NOTIFY motion)
    Q_PROPERTY(QVariantList norm READ norm NOTIFY motion)
    Q_PROPERTY(QVariantList spnav READ spnav NOTIFY motion)
    Q_PROPERTY(qulonglong buttons READ buttons NOTIFY buttonsChanged)
    Q_PROPERTY(QString socketPath READ socketPath CONSTANT)
    Q_PROPERTY(QString version READ version CONSTANT)

public:
    explicit AgentClient(QObject *parent = nullptr);
    void setEngine(QJSEngine *e) { m_engine = e; }
    // Drop JS callbacks before the QML engine is destroyed.
    void shutdown() { m_callbacks.clear(); m_native.clear(); m_engine = nullptr; m_sock.abort(); }

    bool connected() const { return m_sock.state() == QLocalSocket::ConnectedState; }
    QString error() const { return m_error; }
    QVariantMap status() const { return m_status; }
    bool live() const { return m_live; }
    void setLive(bool on);
    QVariantList raw() const { return m_raw; }
    QVariantList norm() const { return m_norm; }
    QVariantList spnav() const { return m_spnav; }
    qulonglong buttons() const { return m_buttons; }
    QString socketPath() const { return m_path; }
    QString version() const { return QStringLiteral(SM_VERSION); }

    // Send `cmd` with `args`; `callback(ok, resultOrErrorMessage)` is invoked
    // with the reply (optional).
    Q_INVOKABLE void call(const QString &cmd, const QVariantMap &args = {}, const QJSValue &callback = QJSValue());
    // The same from C++ (3dmouse-tray has no QML engine).
    using Reply = std::function<void(bool ok, const QVariant &resultOrError)>;
    void callNative(const QString &cmd, const QVariantMap &args, Reply reply);
    Q_INVOKABLE void refresh();
    // Run 3dmousectl (support bundle, diagnostics) and return its output.
    Q_INVOKABLE void runCtl(const QStringList &args, const QJSValue &callback);
    // systemctl --user for the optional session services the GUI may switch on and off
    // (verb: is-enabled, enable, disable; unit: 3dmouse-tray.service or 3dmouse-osd.service).
    Q_INVOKABLE void userUnit(const QString &verb, const QString &unit, const QJSValue &callback);
    Q_INVOKABLE QString toJson(const QVariant &v, bool pretty = true) const;
    Q_INVOKABLE QVariant fromJson(const QString &s) const;
    Q_INVOKABLE QString readFile(const QUrl &url) const;
    Q_INVOKABLE bool writeFile(const QUrl &url, const QString &text) const;

signals:
    void connectedChanged();
    void statusChanged();
    void liveChanged();
    void motion();
    void buttonsChanged();
    void agentEvent(const QString &name, const QVariantMap &body);

private:
    void onReadyRead();
    void handle(const QJsonObject &o);
    void tryConnect();
    void subscribe();
    quint64 send(const QString &cmd, const QVariantMap &args);
    void runProcess(const QString &program, const QStringList &args, const QJSValue &callback);

    QLocalSocket m_sock;
    QTimer m_retry;
    QTimer m_poll;
    QByteArray m_buf;
    QString m_path;
    QString m_error;
    QVariantMap m_status;
    bool m_live = false;
    QVariantList m_raw, m_norm, m_spnav;
    qulonglong m_buttons = 0;
    quint64 m_nextId = 1;
    QHash<quint64, QJSValue> m_callbacks;
    QHash<quint64, Reply> m_native;
    QJSEngine *m_engine = nullptr;
};

#!/bin/sh
# Stability soak monitor (SRS §6: 72 h). Samples the running services once a
# minute and appends CSV; read-only, needs no root.
#
# usage: tools/soak-monitor.sh [hours=72] [csv=$XDG_STATE_HOME/3dmouse-linux/soak.csv]
# run detached:  systemd-run --user --unit=3dmouse-soak tools/soak-monitor.sh 72
# summary:       tools/soak-monitor.sh --summary [csv]
set -eu
STATE=${XDG_STATE_HOME:-$HOME/.local/state}/3dmouse-linux
if [ "${1:-}" = "--summary" ]; then
    CSV=${2:-$STATE/soak.csv}
    exec python3 - "$CSV" <<'PY'
import csv, sys
rows = list(csv.DictReader(open(sys.argv[1])))
if not rows: sys.exit("no samples")
h = (int(rows[-1]["epoch"]) - int(rows[0]["epoch"])) / 3600
def col(k): return [float(r[k]) for r in rows if r[k] not in ("", "-")]
for svc in ("broker", "agent"):
    rss = col(f"{svc}_rss_kb"); cpu = col(f"{svc}_cpu_s")
    restarts = {r[f"{svc}_restarts"] for r in rows}
    pids = {r[f"{svc}_pid"] for r in rows}
    cpu_pct = (cpu[-1] - cpu[0]) / max(1, h * 3600) * 100 if len(cpu) > 1 and len(pids) == 1 else float("nan")
    print(f"{svc:6} RSS {min(rss)/1024:.1f}–{max(rss)/1024:.1f} MB (last {rss[-1]/1024:.1f}), avg CPU {cpu_pct:.3f} %, pids {len(pids)}, restarts {sorted(restarts)}")
gaps = sum(1 for r in rows if r["device"] != "ready")
print(f"{len(rows)} samples over {h:.1f} h; samples without a connected device: {gaps}; locked samples: {sum(r['locked']=='True' for r in rows)}")
PY
fi
HOURS=${1:-72}
CSV=${2:-$STATE/soak.csv}
mkdir -p "$(dirname "$CSV")"
TCK=$(getconf CLK_TCK)
[ -s "$CSV" ] || echo "epoch,broker_pid,broker_restarts,broker_rss_kb,broker_cpu_s,agent_pid,agent_restarts,agent_rss_kb,agent_cpu_s,device,spnav_clients,locked" > "$CSV"
proc() { # pid -> "rss_kb,cpu_s"
    if [ -n "$1" ] && [ "$1" != 0 ] && [ -r "/proc/$1/stat" ]; then
        rss=$(awk '/^VmRSS/{print $2}' "/proc/$1/status")
        cpu=$(awk -v t="$TCK" '{printf "%.2f", ($14+$15)/t}' "/proc/$1/stat")
        echo "$rss,$cpu"
    else echo "-,-"; fi
}
END=$(( $(date +%s) + HOURS * 3600 ))
while [ "$(date +%s)" -lt "$END" ]; do
    bp=$(systemctl show -p MainPID --value 3dmoused.service 2>/dev/null || echo 0)
    br=$(systemctl show -p NRestarts --value 3dmoused.service 2>/dev/null || echo -)
    ap=$(systemctl --user show -p MainPID --value 3dmouse-agent.service 2>/dev/null || echo 0)
    ar=$(systemctl --user show -p NRestarts --value 3dmouse-agent.service 2>/dev/null || echo -)
    st=$(timeout 5 3dmousectl status --json 2>/dev/null | python3 -c '
import json,sys
try:
    d=json.load(sys.stdin); b=d.get("broker") or {}; devs=b.get("devices") or []
    print("%s,%d,%s" % (devs[0].get("state","?") if devs else "none", len(b.get("spnav_clients") or []), d["agent"]["locked"]))
except Exception: print("error,-,-")' )
    echo "$(date +%s),$bp,$br,$(proc "$bp"),$ap,$ar,$(proc "$ap"),$st" >> "$CSV"
    sleep 60
done

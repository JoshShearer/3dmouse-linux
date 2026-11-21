import gi, sys, collections
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
d = Atspi.get_desktop(0)
app = next(d.get_child_at_index(i) for i in range(d.get_child_count()) if "3dmouse-config" in (d.get_child_at_index(i).get_name() or ""))
CTRL = {"button","combo box","check box","spin button","slider","text","toggle button","radio button"}
rows=[]
def ctx(o):
    # nearest preceding label text among siblings, for locating the control in source
    p=o.get_parent()
    if not p: return ""
    idx=o.get_index_in_parent(); lab=""
    for j in range(idx-1, max(-1, idx-4), -1):
        s=p.get_child_at_index(j)
        if s and s.get_role_name() in ("label","heading") and (s.get_name() or "").strip():
            return s.get_name()[:40]
    return f"<{p.get_role_name()} '{(p.get_name() or '')[:30]}'>"
def walk(o):
    try: r=o.get_role_name(); n=(o.get_name() or "").strip()
    except Exception: return
    if r in CTRL:
        p=o.get_parent(); pr=p.get_role_name() if p else ""
        inner = r=="text" and pr in ("spin button","combo box")
        if not inner: rows.append((r, n, ctx(o) if not n else ""))
    for j in range(o.get_child_count()):
        c=o.get_child_at_index(j)
        if c: walk(c)
walk(app)
un=[x for x in rows if not x[1]]
print(f"controls: {len(rows)}, named: {len(rows)-len(un)}, unnamed: {len(un)}")
for r,n,c in un: print(f"  UNNAMED {r:12} near: {c}")

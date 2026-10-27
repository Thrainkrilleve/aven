#!/usr/bin/env python3
"""Independent checker for test_state (PI controller statefulness).

Runs the compiled test, then reproduces the PI recurrence in Python (Kp=0.5,
Ki*dt=0.1, unit step) and checks: live ticks match the reference exactly, the
integral actually RAMPS (statefulness, not a constant), the HOLD tick (7) freezes
the state and publishes the fail-safe, and the controller resumes from the frozen
integral.

Usage: pi_check.py <path-to-test_state.exe>
"""
import sys, subprocess

KP, KIDT, R, Y, SAFE = 0.5, 0.1, 1.0, 0.0, 0.0

def main():
    out = subprocess.run([sys.argv[1]], capture_output=True, text=True).stdout
    rows = []
    for line in out.splitlines():
        d = dict(kv.split("=") for kv in line.split())
        rows.append((int(d["tick"]), int(d["status"]), float(d["u"]), float(d["i"])))

    i_ref = 0.0
    ok = True
    live_u = []
    for tick, status, u, i in rows:
        if tick == 7:   # induced HOLD: fail-safe out, state frozen
            if not (status == 1 and u == SAFE and i == i_ref):
                ok = False; print(f"  tick7 HOLD wrong: status={status} u={u} i={i} (frozen expected {i_ref})")
            continue
        # live reference recurrence
        e = R - Y
        i_ref = i_ref + KIDT * e
        u_ref = KP * e + i_ref
        match = (abs(u - u_ref) == 0.0 and abs(i - i_ref) == 0.0)
        if not match:
            ok = False; print(f"  tick{tick} mismatch: u={u}/{u_ref} i={i}/{i_ref}")
        live_u.append(u)
        print(f"  tick {tick:2d}: u={u:.3f}  i={i:.3f}   (ref u={u_ref:.3f} i={i_ref:.3f})  {'ok' if match else 'MISMATCH'}")

    ramps = all(b > a for a, b in zip(live_u, live_u[1:]))   # strictly increasing => stateful
    print(f"\nmatches reference exactly: {ok}")
    print(f"integral ramps across ticks (stateful, not constant): {ramps}")
    print(f"HOLD froze state @ i=0.6, resumed to i=1.0: "
          f"{abs(rows[5][3]-0.6)<1e-12 and abs(rows[-1][3]-1.0)<1e-12}")
    return 0 if (ok and ramps) else 1

if __name__ == "__main__":
    sys.exit(main())

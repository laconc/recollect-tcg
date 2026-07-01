#!/usr/bin/env python3
"""Drive a REAL PvP match through the running deploy server with two headless
`recollect online join --json` CLI clients — the game half of deploy/smoke.sh.

The server is a black box; the only thing this touches is the `recollect` binary
(spawned twice, once per seat) and its JSON stdout/stdin. It:

  * launches `recollect online join <id> <token> --json` for seat A and seat B,
  * reads each seat's `{"view": <PlayerView>, "legal": [...]}` frames,
  * on a seat's OWN turn echoes a legal move straight back (round-tripping
    `legal[i].cmd`, preferring EndTurn so the match marches to Nightfall), and
  * asserts REDACTION on every frame: a seat's view shows its own hand
    (`you.hand` is a list) but NEVER the opponent's cards (`opponent` carries a
    `hand_count` only — no `hand` array).

It plays until BOTH seats observe a `Finished` view (or the move budget), then prints
exactly one of `GAME_SMOKE_PASS …` / `GAME_SMOKE_FAIL …` (the shell greps for the PASS
marker) and exits 0/1 to match. It does NOT wait for the clients to exit — the
`recollect` CLI keeps its socket open after a finished match, so observing the
terminal view over the wire is the completion signal and the driver then stops the
clients itself. A real handshake + several applied moves + the redaction check is
already an acceptable smoke; both seats reaching a result is the happy path.

Env: CLI_BIN, BASE_URL, MATCH_ID, TOKEN_A, TOKEN_B, MOVE_BUDGET.
"""

import json
import os
import queue
import subprocess
import sys
import threading
import time

CLI_BIN = os.environ["CLI_BIN"]
BASE_URL = os.environ["BASE_URL"]
MATCH_ID = os.environ["MATCH_ID"]
TOKENS = {"A": os.environ["TOKEN_A"], "B": os.environ["TOKEN_B"]}
MOVE_BUDGET = int(os.environ.get("MOVE_BUDGET", "400"))
# Overall wall-clock ceiling so a stuck handshake fails loudly instead of hanging CI.
DEADLINE = time.time() + 120.0


def log(msg):
    print(f"    [game] {msg}", flush=True)


def fail(msg):
    print(f"GAME_SMOKE_FAIL {msg}", flush=True)
    sys.exit(1)


def spawn(seat):
    """One `recollect online join` client, JSON mode, line-buffered."""
    return subprocess.Popen(
        [
            CLI_BIN, "online", "join", MATCH_ID, TOKENS[seat],
            "--json", "--server", BASE_URL,
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        bufsize=1,
    )


def reader(proc, q, seat):
    """Pump a client's stdout lines onto a queue tagged with its seat."""
    for line in proc.stdout:
        q.put((seat, line))
    q.put((seat, None))  # EOF sentinel


def assert_redaction(seat, view):
    """A seat's view shows ITS OWN hand but never the opponent's cards."""
    you = view.get("you", {})
    if not isinstance(you.get("hand"), list):
        fail(f"seat {seat}: own hand missing from its own view (you.hand not a list)")
    opp = view.get("opponent", {})
    # The opponent is counts-only: a `hand_count`, and crucially NO `hand` array.
    if "hand" in opp:
        fail(f"seat {seat}: REDACTION LEAK — opponent.hand present in the view: {opp!r}")
    if "hand_count" not in opp:
        fail(f"seat {seat}: opponent.hand_count missing (view shape changed?): {opp!r}")
    # The redacted view must be the seat's OWN seat, never the opponent's.
    if view.get("seat") != seat:
        fail(f"seat {seat}: received a view for seat {view.get('seat')!r} (redaction/routing)")


def pick_move(legal):
    """Choose a legal move: prefer EndTurn (drives toward Nightfall), else the first.

    Round-trips the server's own `legal[i].cmd` verbatim, so we never construct a
    Command ourselves — whatever the server offered is, by definition, legal.
    """
    for m in legal:
        cmd = m.get("cmd")
        if cmd == "EndTurn":  # the unit-variant serializes to the bare string
            return cmd
    return legal[0]["cmd"] if legal else None


def main():
    procs = {s: spawn(s) for s in ("A", "B")}
    q = queue.Queue()
    for s, p in procs.items():
        threading.Thread(target=reader, args=(p, q, s), daemon=True).start()

    # Latest view + legal menu per seat; whether each has acted at least once.
    last_active = None
    applied_moves = 0
    finished_seats = set()   # seats that have observed a Finished view
    final_result = None
    welcomed = set()
    distinct_hands = {}

    try:
        while time.time() < DEADLINE:
            # Success: BOTH seats observed the terminal view over the wire. We do NOT
            # wait for the clients to EXIT — the `recollect` CLI keeps its socket open
            # after a Finished match (it doesn't self-close), so EOF may never come.
            # Observing the Finished view on both seats IS the proof the game completed;
            # we then stop the clients ourselves (the `finally` below).
            if finished_seats == {"A", "B"}:
                break
            if applied_moves >= MOVE_BUDGET:
                log(f"move budget {MOVE_BUDGET} reached without a result — accepting as a smoke")
                break
            try:
                seat, line = q.get(timeout=30)
            except queue.Empty:
                # No frame for 30s. If a result already landed, that's fine; otherwise
                # the handshake/match genuinely stalled.
                if finished_seats:
                    break
                fail(f"no client output for 30s (handshake stalled?); "
                     f"welcomed={sorted(welcomed)} moves={applied_moves}")
            if line is None:
                # A client closed its socket. Fine once a result landed; premature is a fail.
                if finished_seats:
                    continue
                fail(f"seat {seat} client exited before the match finished")
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue  # non-JSON noise (shouldn't happen on stdout, but be lenient)
            view = msg.get("view")
            if view is None:
                # Rejected / error / non-view frame — surface a rejection loudly.
                if "rejected" in msg:
                    fail(f"seat {seat}: server rejected a move: {msg['rejected']!r}")
                continue

            assert_redaction(seat, view)
            welcomed.add(seat)
            # Record each seat's hand to prove the two seats genuinely differ
            # (a redaction smoke: A's cards are not B's).
            distinct_hands[seat] = json.dumps(view.get("you", {}).get("hand"))

            phase = view.get("phase")
            if isinstance(phase, dict) and "Finished" in phase:
                if seat not in finished_seats:
                    fin = phase["Finished"]
                    final_result = (
                        f"{fin.get('result')} {fin.get('score_a')}-{fin.get('score_b')}"
                    )
                    log(f"seat {seat} sees Finished: {final_result}")
                finished_seats.add(seat)
                # Don't act on a finished seat; loop back to collect the other seat's terminal.
                continue

            active = view.get("active")
            last_active = active
            legal = msg.get("legal", [])
            # Act only on the seat whose turn it is, and only when it's the view's seat.
            if active == seat and legal:
                cmd = pick_move(legal)
                if cmd is None:
                    fail(f"seat {seat}: it is our turn but the legal menu is empty")
                try:
                    procs[seat].stdin.write(json.dumps(cmd) + "\n")
                    procs[seat].stdin.flush()
                except BrokenPipeError:
                    fail(f"seat {seat}: client stdin closed mid-game")
                applied_moves += 1
        else:
            fail("overall deadline reached before a healthy smoke")
    finally:
        for p in procs.values():
            try:
                p.stdin.close()
            except Exception:
                pass
            p.terminate()
        for p in procs.values():
            try:
                p.wait(timeout=5)
            except Exception:
                p.kill()

    # --- Final assertions ---------------------------------------------------------------
    if welcomed != {"A", "B"}:
        fail(f"both seats must hand-shake and receive a redacted view; got {sorted(welcomed)}")
    if applied_moves < 2:
        fail(f"too few moves applied over the wire ({applied_moves}); the match did not advance")
    if len(distinct_hands) == 2 and distinct_hands["A"] == distinct_hands["B"]:
        fail("seats A and B were dealt the SAME hand — redaction/dealing is broken")

    if final_result is not None:
        result = f"reached a result ({final_result})"
    else:
        result = f"advanced {applied_moves} moves (no result within budget)"
    print(
        f"GAME_SMOKE_PASS handshake=AB moves={applied_moves} {result} "
        f"redaction=held last_active={last_active}",
        flush=True,
    )
    sys.exit(0)


if __name__ == "__main__":
    main()

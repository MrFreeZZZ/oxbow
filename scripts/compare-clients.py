#!/usr/bin/env python3
"""Times Oxbow against other Git clients on a Mac, the same way for each: from an action (the app
started, a commit made in a terminal, ↓ in the history) to what the screen shows. It watches the
part of the screen the client's window is put on and notes two times:

  first      the screen first changed (the window, the new row, the selection moved)
  complete   it changed for the last time before staying still for a while (the diff painted)

So it needs nothing from the clients but their windows; docs/speed.md has the steps and how to
read the numbers. macOS only, with python3 from the Xcode tools; nothing to install.

  python3 scripts/compare-clients.py --apps "Oxbow,Fork,Tower" ~/oxbow-perf/history-100k-clone

Terminal needs Screen Recording (to see the window) and Accessibility (to press keys) in
System Settings › Privacy & Security. Results: <out>/results.csv and <out>/summary.md.
"""

import argparse
import csv
import ctypes
import os
import platform
import plistlib
import statistics
import subprocess
import sys
import time
from ctypes import c_bool, c_double, c_long, c_uint16, c_uint32, c_void_p
from pathlib import Path

# A change is when more than this share of the sampled bytes differ: a caret's blink stays under it.
CHANGE = 0.0005
# Every SAMPLE-th byte of a frame is compared; a prime, so it doesn't follow the pixel columns.
SAMPLE = 97
# The screen staying the same this long ends a measurement (longer after a start).
QUIET = 1.0
QUIET_OPEN = 2.0
LIMIT = 30.0

KEYS = {"down": 125, "up": 126}


class Rect(ctypes.Structure):
    _fields_ = [("x", c_double), ("y", c_double), ("width", c_double), ("height", c_double)]


class Screen:
    """The screen through CoreGraphics: a picture of a part of it, and key presses."""

    def __init__(self):
        cg = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices")
        cf = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
        cg.CGWindowListCreateImage.restype = c_void_p
        cg.CGWindowListCreateImage.argtypes = [Rect, c_uint32, c_uint32, c_uint32]
        cg.CGImageGetDataProvider.restype = c_void_p
        cg.CGImageGetDataProvider.argtypes = [c_void_p]
        cg.CGDataProviderCopyData.restype = c_void_p
        cg.CGDataProviderCopyData.argtypes = [c_void_p]
        cg.CGImageRelease.argtypes = [c_void_p]
        cg.CGEventCreateKeyboardEvent.restype = c_void_p
        cg.CGEventCreateKeyboardEvent.argtypes = [c_void_p, c_uint16, c_bool]
        cg.CGEventPost.argtypes = [c_uint32, c_void_p]
        cg.CGPreflightScreenCaptureAccess.restype = c_bool
        cg.CGRequestScreenCaptureAccess.restype = c_bool
        cg.AXIsProcessTrusted.restype = c_bool
        cf.CFDataGetLength.restype = c_long
        cf.CFDataGetLength.argtypes = [c_void_p]
        cf.CFDataGetBytePtr.restype = c_void_p
        cf.CFDataGetBytePtr.argtypes = [c_void_p]
        cf.CFRelease.argtypes = [c_void_p]
        self.cg, self.cf = cg, cf

    def allowed(self):
        """What Terminal still has to be allowed, by the names System Settings gives them."""
        missing = []
        if not self.cg.CGPreflightScreenCaptureAccess():
            self.cg.CGRequestScreenCaptureAccess()
            missing.append("Screen Recording")
        if not self.cg.AXIsProcessTrusted():
            missing.append("Accessibility")
        return missing

    def grab(self, rect):
        """Every SAMPLE-th byte of what the screen shows in `rect`, at one pixel a point."""
        # On screen only, no window left out, nominal resolution (1 << 4): about 10 ms a frame.
        image = self.cg.CGWindowListCreateImage(rect, 1, 0, 1 << 4)
        if not image:
            sys.exit("compare-clients: the screen can't be read; allow Screen Recording for Terminal.")
        data = self.cg.CGDataProviderCopyData(self.cg.CGImageGetDataProvider(image))
        frame = ctypes.string_at(self.cf.CFDataGetBytePtr(data), self.cf.CFDataGetLength(data))[::SAMPLE]
        self.cf.CFRelease(data)
        self.cg.CGImageRelease(image)
        return frame

    def press(self, key):
        for down in (True, False):
            event = self.cg.CGEventCreateKeyboardEvent(None, KEYS[key], down)
            self.cg.CGEventPost(0, event)
            self.cf.CFRelease(event)


def differs(a, b):
    if len(a) != len(b):
        return True
    return sum(x != y for x, y in zip(a, b)) > len(a) * CHANGE


def measure(grab, act, quiet=QUIET, clock=time.perf_counter):
    """Runs `act` and watches the screen: (first, complete) in ms from it, or None for one that
    didn't come in LIMIT seconds; complete is None too when the screen never stays still."""
    last = grab()
    start = clock()
    act()
    first = changed = None
    while True:
        at = clock() - start
        frame = grab()
        if differs(frame, last):
            changed = at
            first = first if first is not None else at
            last = frame
        if first is not None and at - changed >= quiet:
            return round(first * 1000), round(changed * 1000)
        if at > LIMIT:
            return (round(first * 1000) if first is not None else None), None


def osascript(script):
    return subprocess.run(["osascript", "-e", script], capture_output=True, text=True).stdout.strip()


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True)


class Client:
    def __init__(self, name):
        self.name = name
        self.bundle = next((p for p in (Path("/Applications"), Path.home() / "Applications") if (p / f"{name}.app").exists()), None)
        if not self.bundle:
            sys.exit(f"compare-clients: {name}.app is in neither /Applications nor ~/Applications.")
        self.bundle = self.bundle / f"{name}.app"
        with open(self.bundle / "Contents/Info.plist", "rb") as f:
            info = plistlib.load(f)
        self.version = info.get("CFBundleShortVersionString", "?")
        self.process = info.get("CFBundleName", name)

    def running(self):
        return osascript(f'tell application "System Events" to exists process "{self.process}"') == "true"

    def quit(self):
        osascript(f'tell application "{self.name}" to quit')
        for _ in range(100):
            if not self.running():
                return
            time.sleep(0.1)
        subprocess.run(["pkill", "-x", self.process])
        time.sleep(1)

    def launch(self, repo):
        if self.name == "Oxbow":
            # Oxbow reopens the repository it had last; there's no folder to give it.
            config = Path.home() / "Library/Application Support/app.oxbow.desktop"
            config.mkdir(parents=True, exist_ok=True)
            (config / "last-repository").write_text(str(repo))
            subprocess.Popen(["open", "-a", str(self.bundle)])
        else:
            subprocess.Popen(["open", "-a", str(self.bundle), str(repo)])

    def activate(self):
        osascript(f'tell application "{self.name}" to activate')
        time.sleep(0.5)

    def place(self, bounds):
        x, y, w, h = bounds
        for _ in range(50):
            done = osascript(
                f'tell application "System Events" to tell process "{self.process}"\n'
                f"if (count of windows) is 0 then return \"no\"\n"
                f"set position of window 1 to {{{x}, {y}}}\nset size of window 1 to {{{w}, {h}}}\nreturn \"yes\"\nend tell"
            )
            if done == "yes":
                return
            time.sleep(0.2)
        sys.exit(f"compare-clients: {self.name} shows no window to place.")


def hide_others(keep):
    names = " and ".join(f'name is not "{n}"' for n in keep)
    osascript(f'tell application "System Events" to set visible of (every process whose visible is true and {names}) to false')


def stats(times):
    times = sorted(t for t in times if t is not None)
    if not times:
        return "—"
    p95 = times[min(len(times) - 1, max(0, -(-95 * len(times) // 100) - 1))]
    return f"{round(statistics.median(times))} / {p95}"


def main():
    parser = argparse.ArgumentParser(description="Time Git clients the same way, from the screen.")
    parser.add_argument("repo", type=Path, help="the repository, e.g. ~/oxbow-perf/history-100k-clone")
    parser.add_argument("--apps", default="Oxbow,Fork,Tower,GitHub Desktop,Sourcetree,GitKraken", help="names as in /Applications")
    parser.add_argument("--scenarios", default="open,outside,next", help="any of open, outside, next")
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--bounds", default="0,40,1280,800", help="x,y,width,height of the window, in points")
    parser.add_argument("--out", type=Path, default=Path("compare-results"))
    args = parser.parse_args()
    if platform.system() != "Darwin":
        sys.exit("compare-clients: macOS only (on Linux, scripts/acceptance.sh --speed times Oxbow).")

    repo = args.repo.expanduser().resolve()
    bounds = tuple(int(v) for v in args.bounds.split(","))
    rect = Rect(*bounds)
    scenarios = args.scenarios.split(",")
    clients = [Client(name.strip()) for name in args.apps.split(",")]
    screen = Screen()
    missing = screen.allowed()
    if missing:
        sys.exit(f"compare-clients: allow {' and '.join(missing)} for Terminal in System Settings › Privacy & Security, then run it again.")
    terminal = {"Apple_Terminal": "Terminal", "iTerm.app": "iTerm2"}.get(os.environ.get("TERM_PROGRAM", ""), "Terminal")
    grab = lambda: screen.grab(rect)  # noqa: E731

    args.out.mkdir(parents=True, exist_ok=True)
    rows = []
    machine = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string", "hw.memsize"], capture_output=True, text=True).stdout.split("\n")
    git_version = subprocess.run(["git", "--version"], capture_output=True, text=True).stdout.strip()
    print(f"{machine[0]}, {int(machine[1]) // 2**30} GB, macOS {platform.mac_ver()[0]}, {git_version}")
    print(f"Keep {terminal} out of the window's place ({args.bounds}); other apps get hidden.")

    for client in clients:
        print(f"\n{client.name} {client.version}")
        client.quit()
        client.launch(repo)
        time.sleep(8)
        client.place(bounds)
        hide_others([client.process, terminal])

        if "open" in scenarios:
            for run in range(args.runs):
                client.quit()
                time.sleep(2)
                first, complete = measure(grab, lambda: client.launch(repo), QUIET_OPEN)
                rows.append([client.name, client.version, "open", run + 1, first, complete])
                print(f"  open {run + 1}: {first} / {complete} ms")
                client.place(bounds)
                time.sleep(1)

        if "outside" in scenarios:
            client.activate()
            time.sleep(3)
            for run in range(args.runs):
                first, complete = measure(grab, lambda: git(repo, "commit", "-q", "--allow-empty", "-m", f"Outside {client.name} {run + 1}"))
                rows.append([client.name, client.version, "outside commit", run + 1, first, complete])
                print(f"  outside commit {run + 1}: {first} / {complete} ms")

        if "next" in scenarios:
            input(f"  In {client.name}, show the history and click its newest commit, then press Return here.")
            client.activate()
            time.sleep(1)
            for key, label in (("down", "next ↓"), ("up", "back ↑")):
                for run in range(args.runs):
                    first, complete = measure(grab, lambda: screen.press(key))
                    rows.append([client.name, client.version, label, run + 1, first, complete])
                    print(f"  {label} {run + 1}: {first} / {complete} ms")
        client.quit()

    with open(args.out / "results.csv", "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["app", "version", "scenario", "run", "first_ms", "complete_ms"])
        writer.writerows(rows)

    labels = [label for label in ("open", "outside commit", "next ↓", "back ↑") if any(r[2] == label for r in rows)]
    lines = [
        f"{machine[0]}, {int(machine[1]) // 2**30} GB, macOS {platform.mac_ver()[0]}, {git_version}; {repo.name}, {args.runs} runs.",
        "Median / p95 in ms: first change, then complete.",
        "",
        "| App | " + " | ".join(labels) + " |",
        "|---|" + "---|" * len(labels),
    ]
    for client in clients:
        cells = []
        for label in labels:
            mine = [r for r in rows if r[0] == client.name and r[2] == label]
            cells.append(f"{stats([r[4] for r in mine])}, {stats([r[5] for r in mine])}")
        lines.append(f"| {client.name} {client.version} | " + " | ".join(cells) + " |")
    (args.out / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n" + "\n".join(lines))
    print(f"\nSaved in {args.out.resolve()}: results.csv, summary.md")


if __name__ == "__main__":
    main()

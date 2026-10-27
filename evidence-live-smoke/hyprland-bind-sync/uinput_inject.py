#!/usr/bin/env python3
"""Pure-python uinput keyboard + relative mouse injector (group 'input', no root).
libinput hot-plugs the devices; proven driving real overlay windows.
"""
import ctypes, fcntl, os, struct, subprocess, time

UINPUT = "/dev/uinput"
EV_SYN, EV_KEY, EV_REL = 0x00, 0x01, 0x02
UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_RELBIT = 0x40045564, 0x40045565, 0x40045566
UI_DEV_SETUP, UI_DEV_CREATE, UI_DEV_DESTROY = 0x405C5503, 0x5501, 0x5502
REL_X, REL_Y = 0x00, 0x01
BTN_LEFT = 0x110

KEYS = {
    "esc": 1, "1": 2, "2": 3, "3": 4, "4": 5, "5": 6, "6": 7, "7": 8, "8": 9, "9": 10,
    "0": 11, "-": 12, "=": 13, "backspace": 14, "tab": 15,
    "q": 16, "w": 17, "e": 18, "r": 19, "t": 20, "y": 21, "u": 22, "i": 23, "o": 24, "p": 25,
    "leftbrace": 26, "rightbrace": 27, "enter": 28, "leftctrl": 29,
    "a": 30, "s": 31, "d": 32, "f": 33, "g": 34, "h": 35, "j": 36, "k": 37, "l": 38,
    "semicolon": 39, "apostrophe": 40, "grave": 41, "leftshift": 42, "backslash": 43,
    "z": 44, "x": 45, "c": 46, "v": 47, "b": 48, "n": 49, "m": 50, "comma": 51, "dot": 52,
    "slash": 53, "rightshift": 54, "kpasterisk": 55, "leftalt": 56, "space": 57, "capslock": 58,
    "f1": 59, "f2": 60, "f3": 61, "f4": 62, "f5": 63, "f6": 64, "f7": 65, "f8": 66, "f9": 67,
    "f10": 68, "f11": 69, "f12": 70, "delete": 111, "home": 102, "up": 103,
    "pageup": 104, "left": 105, "right": 106, "end": 107, "down": 108, "pagedown": 109,
}
SHIFTED = {"!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8",
           "(": "9", ")": "0", "_": "minus", "+": "equal", ":": "semicolon", '"': "apostrophe",
           "?": "slash", "<": "comma", ">": "dot", "~": "grave", "{": "leftbrace", "}": "rightbrace",
           "|": "backslash"}
EXTRA = {"minus": 12, "equal": 13}


def _write(fd, ev_type, code, value):
    os.write(fd, struct.pack("llHHi", 0, 0, ev_type, code, value))


class Keyboard:
    def __init__(self, name="smoke-uinput-kbd"):
        self.fd = os.open(UINPUT, os.O_WRONLY | os.O_NONBLOCK)
        fcntl.ioctl(self.fd, UI_SET_EVBIT, EV_KEY)
        for code in set(list(KEYS.values()) + list(EXTRA.values())):
            fcntl.ioctl(self.fd, UI_SET_KEYBIT, code)
        setup = struct.pack("HHHH80sI", 3, 0x1234, 0x5678, 1, name.encode(), 0)
        fcntl.ioctl(self.fd, UI_DEV_SETUP, setup)
        fcntl.ioctl(self.fd, UI_DEV_CREATE)
        time.sleep(0.4)

    def tap(self, key, delay=0.05):
        code = KEYS[k] if (k := key.lower()) in KEYS else EXTRA[k]
        _write(self.fd, EV_KEY, code, 1); _write(self.fd, EV_SYN, 0, 0)
        time.sleep(0.02)
        _write(self.fd, EV_KEY, code, 0); _write(self.fd, EV_SYN, 0, 0)
        time.sleep(delay)

    def chord(self, mod, key, delay=0.3):
        m = KEYS[mod]; c = KEYS[key.lower()]
        _write(self.fd, EV_KEY, m, 1); _write(self.fd, EV_KEY, c, 1); _write(self.fd, EV_SYN, 0, 0)
        time.sleep(0.05)
        _write(self.fd, EV_KEY, c, 0); _write(self.fd, EV_KEY, m, 0); _write(self.fd, EV_SYN, 0, 0)
        time.sleep(delay)

    def type(self, text, delay=0.09):
        for ch in text:
            if ch.isupper():
                self.chord("leftshift", ch.lower(), delay)
            elif ch in SHIFTED:
                self.chord("leftshift", SHIFTED[ch], delay)
            elif ch == " ":
                self.tap("space", delay)
            else:
                self.tap(ch, delay)

    def close(self):
        try:
            fcntl.ioctl(self.fd, UI_DEV_DESTROY)
        except OSError:
            pass
        os.close(self.fd)


class Mouse:
    def __init__(self, name="smoke-uinput-mouse"):
        self.fd = os.open(UINPUT, os.O_WRONLY | os.O_NONBLOCK)
        fcntl.ioctl(self.fd, UI_SET_EVBIT, EV_KEY)
        fcntl.ioctl(self.fd, UI_SET_EVBIT, EV_REL)
        fcntl.ioctl(self.fd, UI_SET_KEYBIT, BTN_LEFT)
        fcntl.ioctl(self.fd, UI_SET_RELBIT, REL_X)
        fcntl.ioctl(self.fd, UI_SET_RELBIT, REL_Y)
        setup = struct.pack("HHHH80sI", 3, 0x1234, 0x9abc, 1, name.encode(), 0)
        fcntl.ioctl(self.fd, UI_DEV_SETUP, setup)
        fcntl.ioctl(self.fd, UI_DEV_CREATE)
        time.sleep(0.4)

    def move_rel(self, dx, dy):
        _write(self.fd, EV_REL, REL_X, dx); _write(self.fd, EV_REL, REL_Y, dy)
        _write(self.fd, EV_SYN, 0, 0)
        time.sleep(0.05)

    def click(self):
        _write(self.fd, EV_KEY, BTN_LEFT, 1); _write(self.fd, EV_SYN, 0, 0)
        time.sleep(0.08)
        _write(self.fd, EV_KEY, BTN_LEFT, 0); _write(self.fd, EV_SYN, 0, 0)
        time.sleep(0.25)

    def close(self):
        try:
            fcntl.ioctl(self.fd, UI_DEV_DESTROY)
        except OSError:
            pass
        os.close(self.fd)

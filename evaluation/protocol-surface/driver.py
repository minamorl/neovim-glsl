#!/usr/bin/env python3
"""Measure the Neovim API and UI protocol surface touched by telescope."""

from __future__ import annotations

import argparse
import json
import os
import queue
import shutil
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent
NVIM = os.environ.get("NVIM", "/opt/homebrew/bin/nvim")
HOMEBREW_PATH = "/opt/homebrew/bin:" + os.environ.get("PATH", "")

UI_IMPLEMENTED = {
    "cmdline_block_append",
    "cmdline_block_hide",
    "cmdline_block_show",
    "cmdline_hide",
    "cmdline_pos",
    "cmdline_show",
    "cmdline_special_char",
    "default_colors_set",
    "flush",
    "grid_clear",
    "grid_cursor_goto",
    "grid_destroy",
    "grid_line",
    "grid_resize",
    "grid_scroll",
    "hl_attr_define",
    "hl_group_set",
    "msg_clear",
    "msg_history_clear",
    "msg_history_show",
    "msg_ruler",
    "msg_set_pos",
    "msg_show",
    "msg_showcmd",
    "msg_showmode",
    "popupmenu_hide",
    "popupmenu_select",
    "popupmenu_show",
    "win_close",
    "win_external_pos",
    "win_float_pos",
    "win_hide",
    "win_pos",
}


class Msgpack:
    @staticmethod
    def pack(value):
        if value is None:
            return b"\xc0"
        if value is False:
            return b"\xc2"
        if value is True:
            return b"\xc3"
        if isinstance(value, int):
            if 0 <= value <= 0x7F:
                return bytes([value])
            if -32 <= value < 0:
                return bytes([0x100 + value])
            if 0 <= value <= 0xFF:
                return b"\xcc" + struct.pack(">B", value)
            if 0 <= value <= 0xFFFF:
                return b"\xcd" + struct.pack(">H", value)
            if 0 <= value <= 0xFFFFFFFF:
                return b"\xce" + struct.pack(">I", value)
            if value >= 0:
                return b"\xcf" + struct.pack(">Q", value)
            if -128 <= value:
                return b"\xd0" + struct.pack(">b", value)
            if -32768 <= value:
                return b"\xd1" + struct.pack(">h", value)
            if -2147483648 <= value:
                return b"\xd2" + struct.pack(">i", value)
            return b"\xd3" + struct.pack(">q", value)
        if isinstance(value, str):
            raw = value.encode()
            size = len(raw)
            if size <= 31:
                return bytes([0xA0 | size]) + raw
            if size <= 0xFF:
                return b"\xd9" + struct.pack(">B", size) + raw
            if size <= 0xFFFF:
                return b"\xda" + struct.pack(">H", size) + raw
            return b"\xdb" + struct.pack(">I", size) + raw
        if isinstance(value, (list, tuple)):
            size = len(value)
            body = b"".join(Msgpack.pack(v) for v in value)
            if size <= 15:
                return bytes([0x90 | size]) + body
            if size <= 0xFFFF:
                return b"\xdc" + struct.pack(">H", size) + body
            return b"\xdd" + struct.pack(">I", size) + body
        if isinstance(value, dict):
            items = list(value.items())
            body = b"".join(Msgpack.pack(k) + Msgpack.pack(v) for k, v in items)
            size = len(items)
            if size <= 15:
                return bytes([0x80 | size]) + body
            if size <= 0xFFFF:
                return b"\xde" + struct.pack(">H", size) + body
            return b"\xdf" + struct.pack(">I", size) + body
        raise TypeError(f"cannot msgpack encode {type(value)!r}")

    @staticmethod
    def unpack(stream):
        b = stream.read(1)
        if not b:
            raise EOFError
        code = b[0]
        if code <= 0x7F:
            return code
        if code >= 0xE0:
            return code - 0x100
        if 0x80 <= code <= 0x8F:
            return Msgpack._read_map(stream, code & 0x0F)
        if 0x90 <= code <= 0x9F:
            return [Msgpack.unpack(stream) for _ in range(code & 0x0F)]
        if 0xA0 <= code <= 0xBF:
            return Msgpack._read_str(stream, code & 0x1F)
        if code == 0xC0:
            return None
        if code == 0xC2:
            return False
        if code == 0xC3:
            return True
        if code == 0xC4:
            return Msgpack._read_bytes(stream, Msgpack._read_uint(stream, ">B"))
        if code == 0xC5:
            return Msgpack._read_bytes(stream, Msgpack._read_uint(stream, ">H"))
        if code == 0xC6:
            return Msgpack._read_bytes(stream, Msgpack._read_uint(stream, ">I"))
        if code == 0xC7:
            return Msgpack._read_ext(stream, Msgpack._read_uint(stream, ">B"))
        if code == 0xC8:
            return Msgpack._read_ext(stream, Msgpack._read_uint(stream, ">H"))
        if code == 0xC9:
            return Msgpack._read_ext(stream, Msgpack._read_uint(stream, ">I"))
        if code == 0xCA:
            return Msgpack._read_uint(stream, ">f")
        if code == 0xCB:
            return Msgpack._read_uint(stream, ">d")
        if code == 0xCC:
            return Msgpack._read_uint(stream, ">B")
        if code == 0xCD:
            return Msgpack._read_uint(stream, ">H")
        if code == 0xCE:
            return Msgpack._read_uint(stream, ">I")
        if code == 0xCF:
            return Msgpack._read_uint(stream, ">Q")
        if code == 0xD0:
            return Msgpack._read_uint(stream, ">b")
        if code == 0xD1:
            return Msgpack._read_uint(stream, ">h")
        if code == 0xD2:
            return Msgpack._read_uint(stream, ">i")
        if code == 0xD3:
            return Msgpack._read_uint(stream, ">q")
        if code == 0xD4:
            return Msgpack._read_ext(stream, 1)
        if code == 0xD5:
            return Msgpack._read_ext(stream, 2)
        if code == 0xD6:
            return Msgpack._read_ext(stream, 4)
        if code == 0xD7:
            return Msgpack._read_ext(stream, 8)
        if code == 0xD8:
            return Msgpack._read_ext(stream, 16)
        if code == 0xD9:
            return Msgpack._read_str(stream, Msgpack._read_uint(stream, ">B"))
        if code == 0xDA:
            return Msgpack._read_str(stream, Msgpack._read_uint(stream, ">H"))
        if code == 0xDB:
            return Msgpack._read_str(stream, Msgpack._read_uint(stream, ">I"))
        if code == 0xDC:
            return [Msgpack.unpack(stream) for _ in range(Msgpack._read_uint(stream, ">H"))]
        if code == 0xDD:
            return [Msgpack.unpack(stream) for _ in range(Msgpack._read_uint(stream, ">I"))]
        if code == 0xDE:
            return Msgpack._read_map(stream, Msgpack._read_uint(stream, ">H"))
        if code == 0xDF:
            return Msgpack._read_map(stream, Msgpack._read_uint(stream, ">I"))
        raise ValueError(f"unsupported msgpack code 0x{code:02x}")

    @staticmethod
    def _read_uint(stream, fmt):
        return struct.unpack(fmt, Msgpack._read_bytes(stream, struct.calcsize(fmt)))[0]

    @staticmethod
    def _read_bytes(stream, size):
        data = stream.read(size)
        if len(data) != size:
            raise EOFError
        return data

    @staticmethod
    def _read_str(stream, size):
        return Msgpack._read_bytes(stream, size).decode()

    @staticmethod
    def _read_map(stream, size):
        return {Msgpack.unpack(stream): Msgpack.unpack(stream) for _ in range(size)}

    @staticmethod
    def _read_ext(stream, size):
        ext_type = Msgpack._read_uint(stream, ">b")
        data = Msgpack._read_bytes(stream, size)
        return {"__msgpack_ext_type__": ext_type, "data": data.hex()}


class Nvim:
    def __init__(self):
        self.proc = subprocess.Popen(
            [NVIM, "--embed", "-u", "NONE", "-i", "NONE", "-n"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=None,
            env={**os.environ, "PATH": HOMEBREW_PATH},
        )
        self.messages: "queue.Queue[object]" = queue.Queue()
        self.pending = {}
        self.next_msgid = 1
        self.ui_counts = {}
        self.reader = threading.Thread(target=self._read_loop, daemon=True)
        self.reader.start()

    def close(self):
        try:
            self.command("qa!")
        except Exception:
            pass
        self.proc.kill()
        self.proc.wait()

    def request(self, method, params=None, timeout=10):
        params = [] if params is None else params
        msgid = self.next_msgid
        self.next_msgid += 1
        self._send([0, msgid, method, params])
        return self._wait_response(msgid, timeout)

    def notify(self, method, params=None):
        self._send([2, method, [] if params is None else params])

    def input(self, keys):
        self.notify("nvim_input", [keys])

    def exec_lua(self, code, args=None):
        return self.request("nvim_exec_lua", [code, [] if args is None else args])

    def command(self, command):
        return self.request("nvim_command", [command], timeout=3)

    def ui_attach(self):
        self.request(
            "nvim_ui_attach",
            [
                100,
                36,
                {
                    "rgb": True,
                    "ext_linegrid": True,
                    "ext_multigrid": True,
                    "ext_popupmenu": True,
                    "ext_cmdline": True,
                    "ext_messages": True,
                },
            ],
        )

    def settle(self, seconds):
        time.sleep(seconds)
        self.drain(timeout=0.05)

    def drain(self, timeout=0.05):
        while True:
            try:
                value = self.messages.get(timeout=timeout)
            except queue.Empty:
                return
            if isinstance(value, Exception):
                raise value
            self._collect(value)
            while True:
                try:
                    value = self.messages.get_nowait()
                    if isinstance(value, Exception):
                        raise value
                    self._collect(value)
                except queue.Empty:
                    break

    def _send(self, value):
        data = Msgpack.pack(value)
        assert self.proc.stdin is not None
        self.proc.stdin.write(data)
        self.proc.stdin.flush()

    def _wait_response(self, msgid, timeout):
        if msgid in self.pending:
            err, result = self.pending.pop(msgid)
            if err is not None:
                raise RuntimeError(f"nvim request {msgid} failed: {err!r}")
            return result
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"timed out waiting for nvim response {msgid}")
            value = self.messages.get(timeout=remaining)
            if isinstance(value, Exception):
                raise value
            self._collect(value)
            if msgid in self.pending:
                err, result = self.pending.pop(msgid)
                if err is not None:
                    raise RuntimeError(f"nvim request {msgid} failed: {err!r}")
                return result

    def _collect(self, value):
        if not isinstance(value, list) or not value:
            return
        if value[0] == 1 and len(value) == 4:
            self.pending[value[1]] = (value[2], value[3])
        elif value[0] == 2 and len(value) == 3 and value[1] == "redraw":
            for event in value[2]:
                if isinstance(event, list) and event and isinstance(event[0], str):
                    self.ui_counts[event[0]] = self.ui_counts.get(event[0], 0) + max(len(event) - 1, 0)

    def _read_loop(self):
        assert self.proc.stdout is not None
        try:
            while True:
                self.messages.put(Msgpack.unpack(self.proc.stdout))
        except EOFError:
            return
        except Exception as exc:
            self.messages.put(exc)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", default=str(ROOT / "out/measurement.json"))
    args = parser.parse_args()

    scratch = ROOT / "out/scratch"
    prepare_scratch(scratch)
    plugins = plugin_paths()

    nvim = Nvim()
    try:
        nvim.ui_attach()
        nvim.settle(0.25)
        install_and_load(nvim, scratch, plugins)
        nvim.settle(0.7)
        drive_find_files(nvim)
        drive_file_browser(nvim)
        nvim.drain(timeout=0.25)
        api_calls = json.loads(
            nvim.exec_lua(
                "return vim.json.encode(_G.__protocol_surface_recorder.snapshot())"
            )
        )
        measurement = build_measurement(nvim_version(), scratch, plugins, api_calls, nvim.ui_counts)
        output = Path(args.output)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(measurement, indent=2) + "\n")
        print(output)
    finally:
        nvim.close()


def install_and_load(nvim, scratch, plugins, record_api=True):
    code = r'''
local recorder_path, scratch_dir, plenary, telescope, file_browser, devicons, record_api = ...
local function prepend(path)
  vim.opt.runtimepath:prepend(path)
  vim.opt.packpath:prepend(path)
end

prepend(devicons)
prepend(file_browser)
prepend(telescope)
prepend(plenary)

if record_api then
  local recorder = dofile(recorder_path)
  recorder.install()
  _G.__protocol_surface_recorder = recorder
end
vim.cmd("runtime plugin/telescope.lua")

vim.g.mapleader = " "
vim.api.nvim_set_keymap("n", "<Space>", "<NOP>", { noremap = true, silent = true })
vim.api.nvim_set_keymap("n", "<space>o", "<cmd>Telescope find_files<cr>", { noremap = true, silent = true })
vim.keymap.set("n", "<leader>e", "<cmd>Telescope file_browser path=%:p:h select_buffer=true<CR>", { noremap = true, silent = true })
vim.cmd.cd(vim.fn.fnameescape(scratch_dir))
vim.opt.swapfile = false
vim.opt.hidden = true

local telescope_mod = require("telescope")
telescope_mod.setup({
  defaults = {
    sorting_strategy = "ascending",
    layout_config = { prompt_position = "top" },
  },
  extensions = {
    file_browser = {
      hijack_netrw = true,
      hidden = true,
      respect_gitignore = false,
    },
  },
})
telescope_mod.load_extension("file_browser")
return true
'''
    nvim.exec_lua(
        code,
        [
            str(ROOT / "lua/api_recorder.lua"),
            str(scratch),
            plugins["plenary.nvim"],
            plugins["telescope.nvim"],
            plugins["telescope-file-browser.nvim"],
            plugins["nvim-web-devicons"],
            record_api,
        ],
    )


def drive_find_files(nvim):
    nvim.input("<Space>o")
    nvim.settle(0.8)
    nvim.input("alpha")
    nvim.settle(0.7)
    nvim.input("<C-n>")
    nvim.settle(0.25)
    nvim.input("<CR>")
    nvim.settle(0.9)


def drive_file_browser(nvim):
    nvim.input("<Space>e")
    nvim.settle(0.9)
    nvim.input("move")
    nvim.settle(0.5)
    nvim.input("<C-n>")
    nvim.settle(0.25)
    nvim.input("<Esc>")
    nvim.settle(0.5)


def prepare_scratch(scratch):
    if scratch.exists():
        shutil.rmtree(scratch)
    (scratch / "shader").mkdir(parents=True)
    (scratch / "moving").mkdir()
    (scratch / "alpha.glsl").write_text("void main() { gl_Position = vec4(0.0); }\n")
    (scratch / "beta.glsl").write_text("vec4 beta_color = vec4(1.0);\n")
    (scratch / "shader/water.vert").write_text("#version 450\nvoid main() {}\n")
    (scratch / "shader/lighting.frag").write_text("#version 450\nout vec4 color;\n")
    (scratch / "moving/move_me.glsl").write_text("// candidate for file-browser query\n")
    (scratch / "moving/keep_me.txt").write_text("not selected\n")
    (scratch / "README.md").write_text("# scratch\n")


def plugin_paths():
    lazy = Path.home() / ".local/share/nvim/lazy"
    return {
        "plenary.nvim": str(lazy / "plenary.nvim"),
        "telescope.nvim": str(lazy / "telescope.nvim"),
        "telescope-file-browser.nvim": str(lazy / "telescope-file-browser.nvim"),
        "nvim-web-devicons": str(lazy / "nvim-web-devicons"),
    }


def nvim_version():
    output = subprocess.check_output([NVIM, "--version"], env={**os.environ, "PATH": HOMEBREW_PATH})
    return output.decode().splitlines()[0]


def build_measurement(nvim_version_first_line, scratch, plugins, api_calls, ui_counts):
    observed = []
    implemented_count = 0
    for name, count in sorted(ui_counts.items()):
        implemented = name in UI_IMPLEMENTED
        implemented_count += 1 if implemented else 0
        observed.append(
            {
                "name": name,
                "count": count,
                "candidate_status": "implemented" if implemented else "ignored",
            }
        )
    api = [{"name": row["name"], "count": row["count"]} for row in api_calls]
    return {
        "schema": "neovim-glsl.protocol-surface-measurement/v1",
        "nvim_version_first_line": nvim_version_first_line,
        "scratch_dir": str(scratch),
        "owner_mappings_used": [
            "<space>o -> Telescope find_files",
            "<leader>e -> Telescope file_browser path=%:p:h select_buffer=true",
            "Telescope default <C-n> selection movement",
            "Telescope default <CR> accept / <Esc> close",
        ],
        "plugins": plugins,
        "api": {"distinct_functions": len(api), "calls": api},
        "ui": {
            "attach_options": {
                "rgb": True,
                "ext_linegrid": True,
                "ext_multigrid": True,
                "ext_popupmenu": True,
                "ext_cmdline": True,
                "ext_messages": True,
            },
            "distinct_redraw_events": len(observed),
            "implemented_by_candidate": implemented_count,
            "ignored_by_candidate": len(observed) - implemented_count,
            "observed_events": observed,
        },
        "notes": [
            "The recorder was installed before telescope setup and before the driven picker sessions.",
            "The API recorder stores counts only; it does not record argument values.",
            "The file-browser pass opened the owner's file_browser mapping, typed a query, moved selection with <C-n>, and closed with <Esc>.",
        ],
    }


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"protocol-surface driver failed: {exc}", file=sys.stderr)
        raise

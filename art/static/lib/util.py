"""Small shared helpers."""

import ctypes
import os
import sys
from contextlib import contextmanager


def _flush_c_stdout():
    """Flush the C runtime's stdout buffer (Blender's own printf output)."""
    for name in ("ucrtbase", "msvcrt", None):
        try:
            ctypes.CDLL(name).fflush(None)
            return
        except (OSError, AttributeError, TypeError):
            continue


@contextmanager
def quiet():
    """Silence stdout, Python's and Blender's own (the importers' and
    exporter's INFO lines, the renderer's "Saved:"), for the duration.
    Errors still raise; stderr is untouched."""
    sys.stdout.flush()
    _flush_c_stdout()
    saved = os.dup(1)
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, 1)
    try:
        yield
    finally:
        sys.stdout.flush()
        _flush_c_stdout()
        os.dup2(saved, 1)
        os.close(saved)
        os.close(devnull)

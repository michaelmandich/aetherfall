"""The static model pipeline's modules (art/static/lib), read-only from here.

Rigid units (lib/rigid.py) and Meshy props (lib/props.py) go through its
import, material cleanup and decimation steps. Both pipelines call their
package `lib`, so it is imported under the name `static_lib`.
"""

import importlib
import importlib.util
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
STATIC_LIB = os.path.join(REPO, "art", "static", "lib")


def static_lib(name):
    """A module of art/static/lib: "scene", "materials", "reduce", "gltf"..."""
    if "static_lib" not in sys.modules:
        spec = importlib.util.spec_from_file_location("static_lib", os.path.join(STATIC_LIB, "__init__.py"),
                                                      submodule_search_locations=[STATIC_LIB])
        pkg = importlib.util.module_from_spec(spec)
        sys.modules["static_lib"] = pkg
        spec.loader.exec_module(pkg)
    return importlib.import_module(f"static_lib.{name}")

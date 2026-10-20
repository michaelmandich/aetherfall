"""Meshy props: weapons and shields generated as models of their own.

The Meshy tool writes each to art/units/source/props/<id>.glb (about 3k
triangles, textured) at whatever scale and orientation the model came
out in. Each entry here turns one into the prop frame of lib/props.py:
origin at the main hand's grip, +Y up the haft to the head, +X toward the
striking side. A unit names the id in its PROPS (`"Gear_spear": "short_spear"`);
until the file arrives, the boxy kind named by `like` stands in, and the
build says so.

    like         the boxy kind it replaces: its hand, its mount (turn about
                 the haft in the fist) and the name of its striking part
    length       metres along the haft, butt to head (the file's scale means
                 nothing); match the game's weapon look (weapons.ron), scaled
                 to a 1.65 m figure
    grip         where the main hand holds it: a fraction of the length from
                 the butt (on the middle of the haft there), or (x, y, z)
                 fractions of its oriented bounding box (x strike, y haft,
                 z side): a shield's grip is near (0.0, 0.5, 0.5)
    haft         which of the file's axes, as glTF writes them (+Y up),
                 points up the haft to the head: "+X" ... "-Z"; None takes
                 the longest, pointing the + way, and the build prints which
    strike       the file's axis toward the striking side (blade edge, the
                 way a bow or crossbow shoots, a shield's face); None takes
                 the longer of the other two
    grip_offset  (x, y, z) metres added to the grip in the prop frame (a
                 shield's fist sits a few cm behind its back)
    roll         degrees to turn it about the haft once oriented
    parts        named points: fractions along the haft, or (x, y, z) box
                 fractions; `tip` (1.0) and `butt` (0.0) are always there,
                 and the `like` kind's striking part must be
    triangles    budget (default 1500), texture  largest side, px (default 512)
    glow         emission strength: the model glows in its own colours (the
                 cleanup takes Meshy's emission off everything else)

When a file arrives, build its unit with --sheets and look at the prop in
the hands: set `haft`/`strike` if the build guessed wrong, `grip` if the
hand is in the wrong place.
"""

PROPS = {
    # Bronze Spear: 1.8 m on the 1.4 m spearman's box -> 2.15 m on the model.
    # Meshy's lies along X with the blade at -X (the automatic axis took +X).
    "short_spear": dict(like="spear", length=2.15, grip=0.33, haft="-X"),
    # Crossbow, held by the tiller behind the trigger: the tiller is the haft
    # and the bolt flies toward its head. Meshy's lies flat: tiller along X,
    # butt at +X, prod and stirrup at -X, the prod (0.8 of the length)
    # across Z, its top (where the bolt lies) up +Y.
    "crossbow": dict(like="crossbow", length=0.75, grip=0.36, haft="-X", strike="+Y"),
    # Starborn lightblade, a one-handed blade of solid light: 1.1 m on the
    # 1.6 m lumen knight's box -> about 1.22 m on its 1.8 m model. Not
    # generated yet: check its axes when it arrives.
    "starborn_lightblade": dict(like="lightblade", length=1.22, grip=0.1, glow=3.0),
}

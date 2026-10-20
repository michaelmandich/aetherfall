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
                 points up the haft to the head: "+X" ... "-Z", or (x, y, z)
                 for a model lying askew; None takes the direction the model
                 is longest in (a file axis when within 10 degrees of one),
                 and the build prints which
    flip         True: the automatic haft the other way (the head was at the
                 butt end)
    strike       the file's axis toward the striking side (blade edge, the
                 way a bow or crossbow shoots, a shield's face); None takes
                 the widest direction across the haft
    strike_flip  True: the automatic strike the other way
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
    # 1.6 m lumen knight's box -> about 1.22 m on its 1.8 m model.
    "starborn_lightblade": dict(like="lightblade", length=1.22, grip=0.1, glow=3.0, parts={"edge": 0.7}, flip=True),

    # -- Batch 3. Lengths from each weapon's look (weapons.ron and the
    # factions') scaled from the holder's box to its model, as for the spear;
    # shields from their prompts. The family names the unit's fight clips
    # (lib/fight.py); `like` gives the hand and the striking part's name.
    # Axes checked on renders of every prop normalised (head up, striking
    # side right): `flip` where Meshy put the head at the other end,
    # `strike_flip` where a single edge faced the back; guns have the
    # muzzle as their head and their top as the striking side; shields
    # face glTF +Z.
    # short_bow (bow, archer): 0.85 m on the 1.4 m archer box -> 1.03 m on its 1.7 m model
    "short_bow": dict(like="bow", length=1.03, grip=0.5),
    # sylvan_yew_longbow (bow, sylvan_longbowman): 1.5 m on the 1.45 m sylvan_longbowman box -> 1.76 m on its 1.7 m model
    # Its model holds two bows crossed over each other, welded into one piece: the archer's short bow,
    # scaled to the longbow's length, stands in until it is generated again.
    "sylvan_yew_longbow": dict(like="bow", length=1.76, grip=0.5, file="short_bow"),
    # ironhold_thunder_rune (cast, ironhold_runeguard): 1.0 m on the 1.7 m ironhold_runeguard box -> 1.0 m on its 1.7 m model; a rune axe that throws lightning: held like a staff
    "ironhold_thunder_rune": dict(like="spear", length=1.0, grip=0.15, strike_flip=True),
    # starborn_sunlens (cast, starborn_lightbearer): 1.2 m on the 1.4 m starborn_lightbearer box -> 1.46 m on its 1.7 m model
    "starborn_sunlens": dict(like="spear", length=1.46, grip=0.4),
    # sylvan_crystal_lens (cast, sylvan_lightwarden): 0.9 m on the 1.5 m sylvan_lightwarden box -> 1.02 m on its 1.7 m model
    "sylvan_crystal_lens": dict(like="spear", length=1.02, grip=0.4, flip=True),
    # sylvan_starcall (cast, sylvan_druid): 1.6 m on the 1.5 m sylvan_druid box -> 1.87 m on its 1.75 m model
    "sylvan_starcall": dict(like="spear", length=1.87, grip=0.45),
    # arquebus (gun, arquebusier): 1.1 m on the 1.4 m arquebusier box -> 1.34 m on its 1.7 m model
    "arquebus": dict(like="crossbow", length=1.34, grip=0.3, flip=True),
    # gatling (gun, gatling_crew): 1.2 m on the 1.4 m gatling_crew box -> 1.54 m on its 1.8 m model; fired from the hip
    "gatling": dict(like="crossbow", length=1.54, grip=0.35, flip=True),
    # ironhold_blunderbuss (gun, ironhold_hearthguard): 0.9 m on the 1.35 m ironhold_hearthguard box -> 1.07 m on its 1.6 m model
    "ironhold_blunderbuss": dict(like="crossbow", length=1.07, grip=0.3, flip=True),
    # plasma_carbine (gun, shield_trooper): 1.0 m on the 1.6 m shield_trooper box -> 1.19 m on its 1.9 m model
    "plasma_carbine": dict(like="crossbow", length=1.19, grip=0.35, flip=True),
    # rifle (gun, rifle_squad): 1.1 m on the 1.4 m rifle_squad box -> 1.34 m on its 1.7 m model
    "rifle": dict(like="crossbow", length=1.34, grip=0.3, flip=True),
    # tide_galvanic_harpoon (gun, tide_diver): 1.0 m on the 1.6 m tide_diver box -> 1.25 m on its 2.0 m model; a harpoon gun: stock and pistol grip
    "tide_galvanic_harpoon": dict(like="crossbow", length=1.25, grip=0.3, flip=True),
    # tide_phaser (gun, tide_phaser_marine): 0.9 m on the 1.5 m tide_phaser_marine box -> 1.08 m on its 1.8 m model
    "tide_phaser": dict(like="crossbow", length=1.08, grip=0.3, flip=True),
    # tide_siphon (gun, tide_siphoneer): 0.9 m on the 1.4 m tide_siphoneer box -> 1.12 m on its 1.75 m model; fired from the hip
    "tide_siphon": dict(like="crossbow", length=1.12, grip=0.3, flip=True),
    # ironhold_dane_axe (heavy, ironhold_thane_guard): 1.4 m on the 1.45 m ironhold_thane_guard box -> 1.54 m on its 1.6 m model
    "ironhold_dane_axe": dict(like="sledgehammer", length=1.54, grip=0.1, parts={"face": 0.92}, strike_flip=True),
    # ironhold_mattock (heavy, ironhold_delver_band): 0.7 m on the 1.25 m ironhold_delver_band box -> 0.9 m on its 1.6 m model
    "ironhold_mattock": dict(like="sledgehammer", length=0.9, grip=0.1, parts={"face": 0.92}, flip=True, strike_flip=True),
    # ironhold_piston_hammer (heavy, ironhold_ironclad): 1.0 m on the 1.9 m ironhold_ironclad box -> 1.05 m on its 2.0 m model
    "ironhold_piston_hammer": dict(like="sledgehammer", length=1.05, grip=0.1, parts={"face": 0.92}),
    # shock_maul (heavy, shock_trooper): 0.9 m on the 1.7 m shock_trooper box -> 1.06 m on its 2.0 m model
    "shock_maul": dict(like="sledgehammer", length=1.06, grip=0.1, parts={"face": 0.92}),
    # tide_varangian_axe (heavy, tide_varangian): 1.2 m on the 1.5 m tide_varangian box -> 1.48 m on its 1.85 m model
    "tide_varangian_axe": dict(like="sledgehammer", length=1.48, grip=0.1, parts={"face": 0.92}, flip=True, strike_flip=True),
    # adamant_shield (shield, shield_trooper): 1.2 m tall (its prompt)
    "adamant_shield": dict(like="round_shield", length=1.2, grip=(0.0, 0.5, 0.5), grip_offset=(-0.04, 0.0, 0.0), strike="+Z"),
    # heater_shield (shield, man_at_arms): 0.9 m tall (its prompt)
    "heater_shield": dict(like="round_shield", length=0.9, grip=(0.0, 0.5, 0.5), grip_offset=(-0.04, 0.0, 0.0), strike="+Z"),
    # round_shield (shield, javelineer): 0.8 m tall (its prompt)
    "round_shield": dict(like="round_shield", length=0.8, grip=(0.0, 0.5, 0.5), grip_offset=(-0.04, 0.0, 0.0), strike="+Z"),
    # tide_buckler_shield (shield, tide_marine): 0.4 m tall (its prompt)
    "tide_buckler_shield": dict(like="round_shield", length=0.4, grip=(0.0, 0.5, 0.5), grip_offset=(-0.04, 0.0, 0.0), haft="+Z", strike="+Y"),  # lies flat, face up
    # arming_sword (swing, man_at_arms): 0.9 m on the 1.4 m man_at_arms box -> 1.16 m on its 1.8 m model
    "arming_sword": dict(like="sword", length=1.16, grip=0.1, parts={"edge": 0.75}),
    # club (swing, militia): 0.6 m on the 1.3 m militia box -> 0.78 m on its 1.7 m model
    "club": dict(like="sword", length=0.78, grip=0.12, parts={"edge": 0.85}),
    # dune_khanjar (swing, dune_veil_guard): 0.45 m on the 1.5 m dune_veil_guard box -> 0.51 m on its 1.7 m model
    "dune_khanjar": dict(like="sword", length=0.51, grip=0.25, parts={"edge": 0.75}, flip=True),
    # dune_scimitar (swing, dune_sun_guard): 0.85 m on the 1.45 m dune_sun_guard box -> 1.0 m on its 1.7 m model
    "dune_scimitar": dict(like="sword", length=1.0, grip=0.1, parts={"edge": 0.75}, flip=True, strike_flip=True),
    # starborn_scourge (swing, starborn_zealot): 0.9 m on the 1.35 m starborn_zealot box -> 1.13 m on its 1.7 m model
    "starborn_scourge": dict(like="sword", length=1.13, grip=0.08, parts={"edge": 0.9}, haft=(-0.77, 0.0, -0.64)),
    # tide_boarding_axe (swing, tide_marine): 0.5 m on the 1.3 m tide_marine box -> 0.65 m on its 1.7 m model
    "tide_boarding_axe": dict(like="sword", length=0.65, grip=0.15, parts={"edge": 0.85}, flip=True, strike_flip=True),
    # grenade (throw, grenadier): 0.15 m on the 1.4 m grenadier box -> 0.18 m on its 1.7 m model
    "grenade": dict(like="mallet", length=0.18, grip=(0.5, 0.15, 0.5), parts={"face": 0.5}),
    # javelin (throw, javelineer): 1.2 m on the 1.4 m javelineer box -> 1.46 m on its 1.7 m model; gripped at its balance point
    "javelin": dict(like="spear", length=1.46, grip=0.45, flip=True),
    # sling (throw, slinger): laid out straight, about 0.8 m (its prompt says a metre; the game's
    # look, 0.4 m, is the folded sling); gripped by its finger loop, the pouch in the middle
    "sling": dict(like="sling", length=0.8, grip=0.03, parts={"pouch": 0.5}),
    # halberd (thrust, halberdier): 2.2 m on the 1.4 m halberdier box -> 2.75 m on its 1.75 m model
    "halberd": dict(like="spear", length=2.75, grip=0.3, flip=True, strike_flip=True),
    # pike (thrust, pikeman): 3.2 m on the 1.4 m pikeman box -> 3.89 m on its 1.7 m model; held well back: the butt stays off the ground in the carry
    "pike": dict(like="spear", length=3.89, grip=0.22),
    # engineer_axe (work, engineer): 0.55 m on the 1.3 m engineer box -> 0.72 m on its 1.7 m model; the engineer's Tool_attack (the peasant's axe stroke)
    "engineer_axe": dict(like="work_axe", length=0.72, grip=0.1, parts={"bit": 0.85}, flip=True, strike_flip=True),
}

# Crop presets, fine grid and RGB histogram

The crop menu includes Darktable's built-in aspect ratios, ordered from square
to panoramic. Aspect locks store the physical output ratio as before. Both 2:1
and 65:24 XPan use their exact proportions and can be flipped to portrait.
[Darktable crop presets](https://raw.githubusercontent.com/darktable-org/darktable/master/src/iop/crop.c)

Enable Fine grid in the Grid menu to subdivide the crop into twelfths on each
axis. The subdivision lines draw under the main composition guides. When a
line falls on a main guide, only the stronger guide is drawn. The Grid switch
and G shortcut show or hide both layers together.

The preview histogram plots red, green and blue in sRGB coordinates with one
shared square-root count scale. The traces blend where the distributions
overlap and exclude geometric gaps. The brightest and darkest bins include
values outside the SDR interval.

All 77 release library tests passed. A Wayland editor session verified the RGB
histogram, both guide layers, and the saved crop geometry for 2:1 and XPan.

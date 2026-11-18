# Crop and the corrected canvas

The crop rectangle stays in the same corrected coordinate system used by the
renderer. Its origin may be negative and its size may exceed the original
frame. Rendering evaluates that rectangle against the composed perspective and
lens map in one pass. Opening an existing recipe retains its saved rectangle.

Crop and Guides show the projected footprint. The extent is calculated by
tracing the common RGB sensor boundary through lens correction and the forward
homography. At a crossing of the projective horizon, the footprint is clipped
to a finite depth. Crop also allows 5% beyond each corrected edge as
transparent space for later fill. Reset crop uses the tight footprint.

The Crop buttons and the automatic fit after perspective search the same
corrected footprint and write the same rectangle and aspect lock. Applying a
correction and an automatic fit is one undo step.

The 109 MiB London RAF and a copy of the reported recipe are in the ignored
`.local-fixtures` directory. The RAF checksum is
`65b226c1850a580711e45af1df7bc954193be1dabbdd6fecd5026769d65f156b`.
The real-photo regression samples the left rooftop pavilion above the old
canvas. In the Wayland editor, dragging the top edge of the reported crop to
−0.0895 recovered the roof with the pose and bottom crop edge fixed.
A synthetic editor regression also exercises transparent padding and undo.

All 180 release tests passed, along with the separate London RAF regression.
The README uses three new native editor captures and new full-resolution
comparison exports from that RAF.

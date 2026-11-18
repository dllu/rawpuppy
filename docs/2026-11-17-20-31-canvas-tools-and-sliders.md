# Canvas tools and slider precision

The photo toolbar puts Pan, Crop, Straighten and Guides together. When a tool
changes what a drag does, a strip above the photo identifies the tool, gives the
next action, and offers Done or Cancel. The Before view has a Back to edits
action. Escape returns to Pan. Text fields and open menus keep their keyboard
input, and plain tool shortcuts ignore modifiers and key repeat.

Picking a sample, drawing a level line and drawing a graduated filter return
to Pan after a completed gesture. Crop, perspective guides, clone, heal and
mask painting stay active for multiple gestures. Clone and heal mark the
source on the photo once it is picked. Automatic crop and perspective
corrections preserve the current tool. Find guides enters the guide tool when
requested, so finishing detection cannot reopen it after an exit.

Light and Tone mapping are near the top of the sidebar. The geometry panel
opens the controls for the active geometry tool. AgX has a tone mapper
selector, crop-based automatic levels and an area picker. The common levels and
contrast controls are visible when the section opens. Curve shaping groups the
plot, toe, shoulder and pivot. Advanced endpoints, gamma, look and primaries
have their own sections.

Click a numeric value to type or drag it for fine adjustment. The exposure
track covers ±4 EV and gives more travel to small values around zero. Its
numeric input accepts ±32 EV. AgX levels and contrast, temperature, denoise,
geometry and several colour controls similarly use separate practical track
and numeric ranges.

A headless Wayland editor session verified Crop and its Escape exit, Straighten
and its return to Pan, AgX area selection and its cancel action, and the saved
value of +6.25 EV typed into the exposure field.

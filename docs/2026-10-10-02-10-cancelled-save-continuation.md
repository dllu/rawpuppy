# Cancelled save continuation

The editor workflow review found that Cancel dismissed an unsaved-change request
but left its continue-after-save latch active. A delayed matching save reply could
then consume a later pending open request without that new dialog's explicit Save
decision. Current bytes were already saved, but the cancelled request's approval
should not authorize a different transition.

Cancel now clears both the pending request and its save-continuation latch.
Performing a pending request also clears the latch, including Discard transitions.
The matching snapshot check and normal save/error handling remain in place.

An editor/channel regression queues a snapshot, cancels its continuation, requests
another photograph, and delivers the delayed save reply. The new request remains
pending and no Open work is sent. A fresh explicit Save decision then queues the
current snapshot; its matching reply sends the intended Open and clears pending
continuation state. The test uses the actual editor poll/send logic with controlled
channels and in-memory pixels, without writing a photograph.

All 65 standard tests, strict Rust 1.99 Clippy for all targets, formatting and diff
checks passed. Broader platform and photographic audit items remain open in the
full PROMPT.md goal.

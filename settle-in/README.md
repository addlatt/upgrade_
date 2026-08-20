# settle-in

Runs on Linux at first boot, once.

It looks like a welcome screen. It is a safety gate: it verifies the hardware
actually works — speakers, *not headphones* — confirms the data arrived, and
hands over.

On the safety-copy path it also holds the commit line: the offer to **reclaim**
the Windows partition, made exactly once after verification passes, never
nagged; declining leaves a `reclaim` command behind for later. On the
clean-slate path it tells the user to keep the stick — it is their only backup
until they no longer want one.

It does not teach Linux, install applications, run a tour, or check in later.
The verification is the reason it exists.

Nothing built yet. See `docs/architecture.md`.

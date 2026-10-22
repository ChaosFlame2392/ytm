# Floating Queue Modal and In-Track Seeking

We decided to implement playlist viewing and queue manipulation via a temporary alternate-screen floating modal (using rounded unicode borders and viewport scrolling) and to support relative 5-second in-track seeking via `h`/`l` and `Left`/`Right` arrow keys. This preserves `ytm`'s core design requirement of zero scrollback pollution during normal playback while providing rich arbitrary navigation, two-stage search filtering (`/`), and custom queue building (`Space` to mark, `Enter` to commit).

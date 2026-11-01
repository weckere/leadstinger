# Open Stage Control

`leadstinger.json` is a session for [Open Stage Control](https://openstagecontrol.ammd.net/)
1.x: 16 tile buttons in their tile colours, each with its title and a countdown while it
plays (lit while playing, half-lit while paused), then GO with the next cue's title, Stop
All, a hold-to-talk button, the master fader, the countdown of what's playing, and Sync.

1. In LeadStinger: `leadstinger-cli osc.enable --port 9000 --feedback 127.0.0.1:9001`
2. Start Open Stage Control with this session, sending to LeadStinger and listening for
   its feedback:

   ```bash
   open-stage-control --send 127.0.0.1:9000 --osc-port 9001 --load leadstinger.json
   ```

3. Open the address it prints in any browser, on this machine or a tablet. Titles and
   colours appear within five seconds, or at once with **SYNC**.

Tested with Open Stage Control 1.30.3 (headless server, in a browser) against a running
LeadStinger. Change the layout in Open Stage Control's editor, or edit
`../make-layouts.py` and run it again.

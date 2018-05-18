# Weaver

Weaver is my attempt at a modern reimagining of the command-line, the shell, the terminal, tmux/screen, and ssh.

Very little is currently implemented.  I'm currently impelmenting the server and client in Rust, partly out of personal preference, and partly out of interest in ensuring my ability to keep resource usage respectfully low, especially memory.

Here's a mediocre example of the current state of the project (2018-05-17), to illustrate the general idea of the interaction model I have in mind.

![Weaver Example](./weaverc-example.gif?raw=true)

Some of my goals and aspirations for the project include the following.  I'm not particularly set on any specific details here, and I expect to change my mind about quite a bit as I implement and actually start using it.

# Features
* Accept input in a separate text entry, rather than mixing it into the command history buffer.
* Display process execution, status, and output in interactive UI widgets, instead of a flat text buffer.
* Commands run concurrently; no need to wait for one command to finish before starting another.
* Default to showing just the last (maybe also first?) few lines of output.
* Easily pin persistent running commands (either continually producing output, like tail, or run repeatedly, like watch) to a place on the screen to monitor while you work.
* Interactively select commands from history to see full output, and optionally additional details like time started, time finished, resource usage during execution, exit status, anything else I'm able to record.
* Manage different sessions/windows/groups of command history, like tmux and screen.  Dedicate differnt regions of the display to arbitrary sets of command history.
* All meaningful state is kept in a daemon, viewed by different clients.  Initially just a TUI client, but GUI clients are also planned.
* Store all commands run persistently, along with command output (need some kind of GC or limits on storing output, to avoid filling the disk).
* Able to transparently run commands on remote systems over an SSH tunnel, but all commands, input, interaction, etc. happen on a local client.
* `tail -f webserver-{01..30}:/var/log/mumble/error.log`
* `diff_output @host{1..5} mumblectl --status`
* Shell aliases, utility scripts, environment variables, as much configuration as possible only needs to be done once, for yourself, instead of needing to be distributed across all systems you ever work on, without needing to care if they're out-of-date, shared accounts also used by others, etc.
* All command history from all systems stored locally, so you can search through all history trivially without having to scrape it from many remote hosts.
* Optionally, leave a daemon running on every host you work with, to continue tracking process status while disconnected.
* If using daemons on remote hosts, store-and-forward messages along the network of daemons, to pick up transparently when connectivity is restored after disconnection.
* Track overall system performance information from remote hosts running a daemon.  When desired/configured, display this information for the current (or selected) host (or hosts) in a panel in a dedicated region of the display, either as current values or a sparkline graph over time.
* Transparently copy files or pipe input/output between any connected hosts, regardless of topology, without needing to manually route between bastion hosts, etc.
* Scripting languae that can be used to run commands on the entire connected network of systems.
* Integrations with programming languages and other interactive systems to act as a repl.

I am confident in impelemnting the technical backend of thie system, but I feel very intimidated and underconfident in my ability to make good UI and UX choices.  I am extremely interested in hearing from anyone who has either strong or detailed preferences about what specific user interactions should be taken to either perform or communicate the results of any of the above, or any other feature that you'd be interested in seeing as part of this work.


# TODO

* Make better screen recording
* Command prefix for UI interaction
* Implement generic scrolling container widget for text-ui
* Implement scrolling through command output, and command history
* Search through commands and their output
* Track sessions as arbitrary tags on command history items
* Toggle between different layouts of arbitrary subsets of command history (windows/sessions/splits)
* Lazy load command history on-demand, rather than slurping the entire command history up into the client
* Add RPC-style messages, for responding to specific requests?
* Persist command history to disk
* Establish SSH connections
* Start using a shell language parser of some kind, rather than execing bash
* User alias configuration
* User-configurable style/theme

# 20. Pods

One `suture serve` process is one point of failure. Several pods over one state directory keep the service up when one of them stops, and share out the requests to adapters among them.

## Starting pods

Give each pod a name, and the same state directory, on a volume they all reach:

```sh
suture serve examples/tickets --state /data/tickets --pod a --listen 0.0.0.0:7878
suture serve examples/tickets --state /data/tickets --pod b --listen 0.0.0.0:7879
```

```text
tickets v1 on http://0.0.0.0:7879, state in /data/tickets, pod b
  with pods a
```

The gateway may send any input to any pod. `GET /health` names the pod and the pods it sees live:

```text
{"ok": true, "module": "tickets", "version": 1, "pod": "b", "pods": ["a", "b"]}
```

Without `--pod`, a process is the pod `solo`, and follows the same rules. So two processes started on one state directory by mistake take turns instead of corrupting the log.

## One log

The log is still the whole state. A pod appends to it only while it holds the lock on `log.lock`. While it holds the lock, it first applies what the other pods appended since it last looked, then its own input, and writes that input to the log before letting go. Every pod applies every line, in the log's order, so every pod holds the same units. A restart, or a new pod, replays the log and comes to the same state too.

A read (`/units`, `/trace`, `/pending`) first catches up with the log, so it sees every input another pod has acknowledged.

The secrets in `secrets.json` (chapter 19) are written under the same lock, before the line that refers to them, so every pod finds them.

## Owners

Each unit has one owner among the live pods, chosen by a hash of its label. Only the owner posts the unit's requests to adapters. With pods `a` and `b`, about half the bookings are charged through each.

A pod beats every second, writing the time into `pods/⟨name⟩`. A pod whose last beat is 10 seconds old is no longer live: its units go to the others, which post their waiting requests. A pod that stops cleanly, on `SIGTERM` or `SIGINT`, removes its file, and its units move at once. Chapter 22 runs pods as containers.

A pod that cannot write the log stops taking inputs and stops beating, so its units go to the others; restart it, and it replays the log.

During a handover, two pods may both post the same request. A request carries its key (chapter 17), and an adapter must answer the same key once. This was already so across a restart.

## Timers

Any pod may fire the timers that fell due. It does so while holding the lock, after catching up, so a timer another pod has fired does not fire again.

## Deploys

Start one pod with the new version (chapter 18). It deploys it under the lock, and the other pods apply the deploy when they next read the log. Then restart the others with the new version too. A pod started with an older version than the log's is refused, as before.

## What this is not

Every pod holds all of the state and applies every input, so pods do not make a program with many units faster: they make it stay up, and they share out the adapter traffic. The state directory must be on a volume every pod reaches, with locks that work across them (a local disk, or a network file system with `flock` support). Chapter 16 lists what comes later.

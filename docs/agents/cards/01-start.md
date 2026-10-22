# start: how to work on a Suture module, the loop and the files

A module is the `.st` files of one directory. Scenarios are `.scenario` files beside them. Work in this loop:

1. Write or change the `.st` files.
2. Run `suture check <dir>`. It compiles, runs every scenario beside the program, and prints `✓` for each passing scenario, the trace and failures of each failing one, and every compile error with its file and line. Exit 0 means all compile and pass. `--json` gives the same as JSON.
3. Fix the first error, then check again. An error names the form it expected; `suture help <topic>` shows the forms.
4. When it passes, `suture build <dir>` prints the read-back: each rule in plain English, its cost, who may do what. Show that to the person who asked; it is what they review.

Every module starts with one `module` line. Statements are one per line; a long one continues on a more indented line. `--` starts a comment.

```suture
module rooms version 1, currency EUR

role staff

kind Room    { name: Text, seats: Count }
kind Booking { room: Room, people: Count, cancelled: Bool = no }

rule fits: every Booking's people is at most its room's seats

when a staff cancels a Booking: the Booking is cancelled

admit add a new Room by staff
admit book a new Booking by staff
admit cancel Booking by staff
```

```scenario
scenario "a booking that does not fit is refused":
  given  a Room R with name "Oak" and seats 4
  when   staff Sam books a new Booking B1 with room R and people 3
  and    staff Sam books a new Booking B2 with room R and people 6
         is refused by rule fits
  expect B1's people is 3
```

Topics: `suture help kinds`, `gate`, `rules`, `parts`, `time`, `recipes`, `scenarios`, `models`, `api`, `limits`. `suture help all` prints every card.

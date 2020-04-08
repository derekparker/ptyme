# ptyme

(pronounced PTY me, kinda like "beer me, please")

ptyme is a lightweight, portable (across UNIX systems) program to create 
a PTY pair and then proxy to it.

## Usage

Simply run the program:

```bash
$ ptyme
```

Once the program has started it will print out the name of the PTY slave:

```bash
Opened new PTY device: /dev/ttys009
```
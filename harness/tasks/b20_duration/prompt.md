Issue: parse_duration fails on values in seconds such as '45s'.

The code is in lib/time_fmt.py. Fix it so that `sh check.sh` passes. Keep the change minimal and do not edit check.sh or check.py. When you finish, run `sh check.sh` and state its exact final output.

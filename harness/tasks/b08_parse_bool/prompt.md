Issue: parse_bool rejects 'TRUE' and 'False', which should be accepted case-insensitively.

The code is in lib/conv.py. Fix it so that `sh check.sh` passes. Keep the change minimal and do not edit check.sh or check.py. When you finish, run `sh check.sh` and state its exact final output.

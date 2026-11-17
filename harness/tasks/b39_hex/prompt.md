Issue: hex_color drops the leading zero of channels below 16.

The code is in lib/color.py. Fix it so that `sh check.sh` passes. Keep the change minimal and do not edit check.sh or check.py. When you finish, run `sh check.sh` and state its exact final output.

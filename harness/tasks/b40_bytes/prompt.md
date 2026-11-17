Issue: bytes_human uses a threshold of 1000 instead of 1024 and so shows 1000 bytes as 1.0 KB.

The code is in lib/fmt.py. Fix it so that `sh check.sh` passes. Keep the change minimal and do not edit check.sh or check.py. When you finish, run `sh check.sh` and state its exact final output.

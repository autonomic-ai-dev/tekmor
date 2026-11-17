Issue: version_newer compares versions as text, so '1.10' sorts before '1.9'.

The code is in lib/versions.py. Fix it so that `sh check.sh` passes. Keep the change minimal and do not edit check.sh or check.py. When you finish, run `sh check.sh` and state its exact final output.

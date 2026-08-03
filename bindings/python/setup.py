"""setuptools entry point — pins the wheel platform tag.

The wheel bundles Windows x64 DLLs (mediaway/_native/*.dll), but setuptools
sees no compiled extension modules and would tag the wheel `py3-none-any`.
`bdist_wheel.plat_name` forces `py3-none-win_amd64` so PyPI refuses to serve
it to non-Windows interpreters.
"""

from setuptools import setup

setup(options={"bdist_wheel": {"plat_name": "win_amd64"}})

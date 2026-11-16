"""WP-T4 异构黑盒夹具：被测对象 = CI 构建的 release 编译产物（非进程内 API）。"""
import os
import pathlib
import subprocess
import tempfile

import pytest

BIN = os.environ.get(
    "RUPOST_BIN",
    str(pathlib.Path(__file__).resolve().parent.parent / "target" / "release" / "rupost"),
)


@pytest.fixture
def run():
    """在隔离工作区/历史目录中执行 rupost 编译产物（超时 60s 防挂死）。"""
    ws = tempfile.TemporaryDirectory()
    hist = tempfile.TemporaryDirectory()

    def _run(*args):
        return subprocess.run(
            [BIN, *args],
            cwd=ws.name,
            env={**os.environ, "RUPOST_HISTORY_DIR": hist.name},
            capture_output=True,
            text=True,
            timeout=60,
        )

    yield _run
    ws.cleanup()
    hist.cleanup()

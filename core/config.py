import os
import shutil
from pathlib import Path
from typing import Optional

# Base Paths
BASE_DIR = Path(__file__).resolve().parent.parent
BIN_DIR = BASE_DIR / "bin"
DOWNLOADS_DIR = Path(os.environ.get("USERPROFILE", str(Path.home()))) / "Downloads" / "OpenIDM"
DB_PATH = BASE_DIR / "downloads.db"

# Ensure bin dir exists and is in PATH
BIN_DIR.mkdir(parents=True, exist_ok=True)
bin_dir_str = str(BIN_DIR)
if bin_dir_str not in os.environ.get("PATH", ""):
    os.environ["PATH"] = bin_dir_str + os.pathsep + os.environ.get("PATH", "")

def get_ffmpeg_path() -> Optional[str]:
    # 1. Local open-idm/bin/ffmpeg.exe
    local_ffmpeg = BIN_DIR / "ffmpeg.exe"
    if local_ffmpeg.is_file():
        return str(local_ffmpeg)

    # 2. System PATH
    which_ffmpeg = shutil.which("ffmpeg")
    if which_ffmpeg:
        return which_ffmpeg

    # 3. imageio_ffmpeg library
    try:
        import imageio_ffmpeg
        exe = imageio_ffmpeg.get_ffmpeg_exe()
        if os.path.isfile(exe):
            return exe
    except Exception:
        pass

    return None

def get_node_path() -> Optional[str]:
    return shutil.which("node")

# Server Settings
SERVER_HOST = "127.0.0.1"
SERVER_PORT = 6899

# Download Engine Settings
DEFAULT_CHUNKS = 8  # Number of parallel connections for direct downloads
MAX_CONCURRENT_DOWNLOADS = 3
CHUNK_BUFFER_SIZE = 64 * 1024  # 64 KB buffer

# Ensure default folders exist
DOWNLOADS_DIR.mkdir(parents=True, exist_ok=True)
(DOWNLOADS_DIR / "Video").mkdir(parents=True, exist_ok=True)
(DOWNLOADS_DIR / "Music").mkdir(parents=True, exist_ok=True)
(DOWNLOADS_DIR / "Documents").mkdir(parents=True, exist_ok=True)
(DOWNLOADS_DIR / "Programs").mkdir(parents=True, exist_ok=True)
(DOWNLOADS_DIR / "Compressed").mkdir(parents=True, exist_ok=True)
(DOWNLOADS_DIR / "General").mkdir(parents=True, exist_ok=True)

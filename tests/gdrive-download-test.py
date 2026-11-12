"""Drive regressions: folders, confirmation, errors and complete file transfer."""
import io
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import requests

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from core.gdrive_handler import GoogleDriveHandler as Drive, GoogleDriveFolderError
from core.segmented_downloader import DownloadTask

FILE_ID = "123456789012345678901234567890123"
URL = f"https://drive.usercontent.google.com/download?id={FILE_ID}&export=download&confirm=t"


def response(body, status=200, content_type="application/pdf", disposition='attachment; filename="manual.pdf"'):
    result = requests.Response()
    result.status_code = status
    result.url = URL
    result.headers.update({"Content-Type": content_type, "Content-Length": str(len(body))})
    if disposition:
        result.headers["Content-Disposition"] = disposition
    result.raw = io.BytesIO(body)
    return result


class DriveTests(unittest.TestCase):
    def test_folder_mime_is_never_a_file_download(self):
        node = [None] * 36
        node[0] = [None, FILE_ID]
        node[4] = "application/vnd.google-apps.folder"
        node[35] = [[["Wheel Loader Group"]]]
        entries = []
        Drive._walk_and_extract_files(node, entries)
        self.assertTrue(entries[0]["is_folder"])
        self.assertIn("/drive/folders/", entries[0]["download_url"])

    def test_folder_only_listing_can_be_opened(self):
        page = response(b"<title>Group - Google Drive</title>", content_type="text/html", disposition="")
        folder = {"id": FILE_ID, "is_folder": True, "name": "Group"}
        with patch("core.gdrive_handler.requests.Session") as session, patch.object(Drive, "_extract_files_from_html", return_value=[folder]):
            session.return_value.get.return_value = page
            info = Drive.get_folder_info(FILE_ID, URL)
        self.assertEqual(info["files"], [])
        self.assertEqual(info["folders"], [folder])

    def test_confirmation_keeps_all_fields_and_uses_same_session(self):
        warning = response(b'''<form action="https://drive.usercontent.google.com/download?existing=1" id="download-form">
            <input value="t" name="confirm"><input name="id" value="file-id">
            <input name="uuid" value="token&amp;value"><input name="resourcekey" value="key"></form>''',
            content_type="text/html", disposition="")
        download = response(b"%PDF-real-content")
        session = Mock()
        session.get.side_effect = [warning, download]
        result = Drive.open_file(FILE_ID, URL, session, {"Range": "bytes=0-0"})
        self.assertIs(result, download)
        next_url = session.get.call_args_list[1].args[0]
        self.assertIn("existing=1", next_url)
        self.assertIn("uuid=token%26value", next_url)
        self.assertIn("resourcekey=key", next_url)
        self.assertNotIn("Range", session.get.call_args_list[0].kwargs["headers"])
        result.close()

    def test_html_error_is_rejected_instead_of_becoming_metadata(self):
        session = Mock()
        session.get.return_value = response(b"Access denied", 403, "text/html", "")
        with self.assertRaisesRegex(RuntimeError, "HTTP 403"):
            Drive.open_file(FILE_ID, URL, session)

    def test_folder_disguised_as_file_has_actionable_error(self):
        session = Mock()
        session.get.side_effect = [response(b"Not found", 404, "text/html", ""),
                                   response(b'application/vnd.google-apps.folder', content_type="text/html", disposition="")]
        with self.assertRaisesRegex(GoogleDriveFolderError, "folder Google Drive"):
            Drive.open_file(FILE_ID, URL, session)

    def test_utf8_filename_and_exact_size(self):
        result = response(b"%PDF-data", disposition="attachment; filename=manual.pdf; filename*=UTF-8''panduan%20mesin.pdf")
        info = Drive.file_metadata(result, FILE_ID, URL)
        self.assertEqual(info["filename"], "panduan mesin.pdf")
        self.assertEqual(info["filesize"], 9)
        result.close()

    def test_drive_download_bypasses_video_engine_and_corrects_bin_name(self):
        with tempfile.TemporaryDirectory() as directory:
            task = DownloadTask("drive-test", URL, "manual", "manual.bin", filepath=str(Path(directory) / "manual.bin"))
            payload = b"%PDF-1.7\n" + bytes(range(256)) * 8192
            # A previous interrupted download must never be promoted by the video recovery heuristic.
            Path(task.filepath + ".part").write_bytes(payload[:1500000])
            with patch.object(Drive, "open_file", return_value=response(payload)), patch.object(task, "_download_with_ytdlp") as video:
                task._run()
            self.assertEqual(task.status, "completed", task.error_message)
            self.assertEqual(Path(task.filepath).read_bytes(), payload)
            self.assertEqual(task.filename, "manual.pdf")
            self.assertEqual(task.downloaded_bytes, len(payload))
            video.assert_not_called()

    def test_incomplete_file_never_gets_completed_filename(self):
        with tempfile.TemporaryDirectory() as directory:
            task = DownloadTask("drive-test", URL, "manual", "manual.pdf", filepath=str(Path(directory) / "manual.pdf"))
            result = response(b"partial")
            result.headers["Content-Length"] = "100"
            with patch.object(Drive, "open_file", return_value=result):
                task._run()
            self.assertEqual(task.status, "error")
            self.assertFalse(Path(task.filepath).exists())
            self.assertTrue(Path(task.filepath + ".part").exists())


if __name__ == "__main__":
    unittest.main()

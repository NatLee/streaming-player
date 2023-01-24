from requests import get
from yt_dlp import YoutubeDL

YDL_OPTIONS = {
    "quiet": True,
    "format": "best",
    "noplaylist": True,
    "skip_download": True,
}


def search(arg):
    with YoutubeDL(YDL_OPTIONS) as ydl:
        try:
            get(arg)
        except:
            video = ydl.extract_info(f"ytsearch:{arg}", download=False)["entries"][0]
        else:
            video = ydl.extract_info(arg, download=False)

    return video


def get_youtube_video_info(youtube_video_url):
    """
    使用 `youtube_dl` 來獲取影片資訊
    日後可能需要自己 parse 或是拆解它的做法來優化流程
    """
    info = search(youtube_video_url)
    title = info.get("title")
    duration = info.get("duration")
    webpage_url = info.get("webpage_url")  # unique url
    video_id = info.get("id")
    video_url = info.get("url")

    return {
        "status": "ok",
        "title": title,
        "duration": duration,
        "webpage_url": webpage_url,
        "video_id": video_id,
        "video_url": video_url,
    }

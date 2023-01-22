
from  youtube_dl import YoutubeDL

def get_youtube_video_info(youtube_video_url):
    """
    使用 `youtube_dl` 來獲取影片資訊
    日後可能需要自己 parse 或是拆解它的做法來優化流程
    """
    options = {
        'quiet': True,
        'forceurl': True,
        'skip_download': True,
        'format': 'best'
    }
    with YoutubeDL(options) as ydl:
        info = ydl.extract_info(
            youtube_video_url,
            download=False
        )
        title = info.get('title')
        duration = info.get('duration')
        video_id = info.get('id')
        video_url = info.get('url')
    return {
        'title': title,
        'duration': duration,
        'video_id': video_id,
        'video_url': video_url
    }





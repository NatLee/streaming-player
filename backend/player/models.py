from django.db import models
from django.urls import reverse

from simple_history.models import HistoricalRecords


class Playlist(models.Model):
    song_name = models.CharField("歌名", max_length=255)
    url = models.CharField("URL", max_length=512, unique=True)
    created_at = models.DateTimeField("新增時間", auto_now_add=True)
    last_played_at = models.DateTimeField("最後播放時間", auto_now=True)
    duration = models.IntegerField("播放秒數", default=0)

    favorite = models.BooleanField("是否收藏", default=False)
    block = models.BooleanField("是否黑名單", default=False)

    history = HistoricalRecords(table_name="playlist_history")

    def __str__(self):
        return f"{self.song_name} - {self.duration//60} min(s)"

    class Meta:
        db_table = "playlist"
        verbose_name_plural = "歌曲清單"


class PlaylistOrderHistory(models.Model):
    playlist = models.ForeignKey(Playlist, verbose_name="歌名", on_delete=models.CASCADE)
    user = models.CharField("點歌者", max_length=128)
    created_at = models.DateTimeField("新增時間", auto_now_add=True)
    played = models.BooleanField("是否播放完畢", default=False)
    cut = models.BooleanField("是否被卡", default=False)
    autoplay = models.BooleanField("是否爲自動點播", default=False)

    def __str__(self):
        return f"{self.playlist} - {self.user}"

    class Meta:
        db_table = "playlist_order_history"
        verbose_name_plural = "點歌歷史"


class PlaylistOrderQueue(models.Model):
    playlist_order = models.ForeignKey(
        PlaylistOrderHistory, verbose_name="待播放歌名", on_delete=models.CASCADE
    )
    created_at = models.DateTimeField("新增時間", auto_now_add=True)
    order = models.SmallIntegerField("播放順序（越大越晚播）", default=0)

    def __str__(self):
        return f"{self.order}"

    def to_dict(self):
        return {
            "id": self.id,
            "song_name": self.playlist_order.playlist.song_name,
            "url": self.playlist_order.playlist.url,
            "user": self.playlist_order.user,
            "order": self.order,
        }

    class Meta:
        db_table = "playlist_order_queue"
        verbose_name_plural = "點歌佇列"


from django.db import models
from django.urls import reverse

from simple_history.models import HistoricalRecords

class Playlist(models.Model):
    song_name = models.CharField("歌名", max_length=255)
    url = models.CharField("URL", max_length=512)
    created_at = models.DateTimeField("新增時間", auto_now_add=True)
    last_played_at = models.DateTimeField("最後播放時間", auto_now=True)

    history = HistoricalRecords(table_name="playlist_history")

    def __str__(self):
        return self.song_name

    def get_absolute_url(self):
        """Returns the url to access a detail record for this model."""
        return reverse("playlist-detail", args=[str(self.id)])

    class Meta:
        db_table = "playlist"
        verbose_name_plural = "播放列表"

class PlaylistOrderHistory(models.Model):
    playlist = models.ForeignKey(Playlist, verbose_name='歌名', on_delete=models.CASCADE)
    user = models.CharField("點歌者", max_length=128)
    created_at = models.DateTimeField("新增時間", auto_now_add=True)
    played = models.BooleanField("是否播放", default=False)

    def __str__(self):
        return f'{self.playlist} - {self.user}'

    class Meta:
        db_table = "playlist_order_history"
        verbose_name_plural = "點歌歷史"



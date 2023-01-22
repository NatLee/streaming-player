import datetime
from django.contrib import admin
from rangefilter.filters import DateRangeFilter, DateTimeRangeFilter, NumericRangeFilter

from simple_history.admin import SimpleHistoryAdmin

from player.models import (
    Playlist,
    PlaylistOrderHistory,
)

def get_last_week():
    return datetime.date.today()-datetime.timedelta(days=7)

@admin.register(Playlist)
class PlaylistAdmin(SimpleHistoryAdmin):
    list_display = (
        "id",
        "song_name",
        "url",
        "duration",
        "last_played_at",
        "created_at",
    )
    list_filter = (
        ('created_at', DateRangeFilter),
    )
    search_fields = ["song_name", "url"]

    def get_rangefilter_created_at_default(self, request):
        return (get_last_week, datetime.date.today)


@admin.register(PlaylistOrderHistory)
class PlaylistOrderHistoryAdmin(admin.ModelAdmin):
    list_display = (
        "id",
        "playlist",
        "user",
        "played",
        "created_at",
    )
    list_filter = (
        ('created_at', DateRangeFilter),
    )
    search_fields = ["playlist__song_name", "playlist__url", "user"]

    def get_rangefilter_created_at_default(self, request):
        return (get_last_week, datetime.date.today)




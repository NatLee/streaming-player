import datetime
from django.contrib import admin
from django.utils.html import format_html

from rangefilter.filters import DateRangeFilter, DateTimeRangeFilter, NumericRangeFilter

from simple_history.admin import SimpleHistoryAdmin

from player.models import Playlist, PlaylistOrderHistory, PlaylistOrderQueue


def get_last_week():
    return datetime.date.today() - datetime.timedelta(days=7)


@admin.register(Playlist)
class PlaylistAdmin(SimpleHistoryAdmin):
    list_display = (
        "id",
        "song_name",
        "url",
        "video_link",
        "duration",
        "favorite",
        "block",
        "missing",
        "last_played_at",
        "created_at",
    )
    list_filter = (
        "favorite",
        "block",
        "missing",
    )
    search_fields = ["song_name", "url"]

    def get_rangefilter_created_at_default(self, request):
        return (get_last_week, datetime.date.today)

    def video_link(self, obj):
        return format_html('<a href="{}" target="_blank">Link</a>', obj.url)
    video_link.short_description = 'Video Link'


@admin.register(PlaylistOrderHistory)
class PlaylistOrderHistoryAdmin(admin.ModelAdmin):
    list_display = (
        "id",
        "playlist",
        "user",
        "played",
        "cut",
        "autoplay",
        "created_at",
    )
    list_filter = (
        "played",
        "cut",
        ("created_at", DateRangeFilter),
    )
    search_fields = ["playlist__song_name", "playlist__url", "user"]

    def get_rangefilter_created_at_default(self, request):
        return (get_last_week, datetime.date.today)


@admin.register(PlaylistOrderQueue)
class PlaylistOrderQueueAdmin(admin.ModelAdmin):
    ordering = ("order",)
    list_display = (
        "id",
        "order",
        "get_user",
        "playlist_order",
        "created_at",
    )
    list_filter = (("created_at", DateRangeFilter),)
    search_fields = [
        "playlist_order__playlist__song_name",
        "playlist_order__playlist__url",
        "playlist_order__user",
    ]

    def get_rangefilter_created_at_default(self, request):
        return (get_last_week, datetime.date.today)

    @admin.display(ordering="playlist_order__user", description="點歌者")
    def get_user(self, obj):
        return obj.playlist_order.user

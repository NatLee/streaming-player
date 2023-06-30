from django.urls import path
import logging

from player import views

logger = logging.getLogger(__name__)

urlpatterns = [
    path("", views.dashboard, name="player"),
    path("login", views.login, name="player-login"),
    path(
        "youtube-video-info",
        views.YoutubeVideoInfo.as_view(),
        name="youtube_video_info",
    ),
    path(
        "nightbot/order",
        views.NightbotOrder.as_view(),
        name="nightbot_order",
    ),
    path(
        "nightbot/delete",
        views.NightbotDeleteFromQueue.as_view(),
        name="nightbot_delete",
    ),
    path(
        "nightbot/current",
        views.NightbotCurrentSongInQueue.as_view(),
        name="nightbot_current_song_in_queue",
    ),
    path(
        "nightbot/order/<str:user>/count",
        views.NightbotUserCountRecord.as_view(),
        name="nightbot_user_count_record",
    ),
    path(
        "nightbot/current/poll",
        views.NightbotUserPollRemoveNowPlayingSong.as_view(),
        name="nightbot_user_poll_remove_playing_song",
    ),
    path(
        "nightbot/<int:song_pk_in_queue>/insert",
        views.NightbotUserPollInsertSongToTop.as_view(),
        name="nightbot_user_poll_insert_song_to_top",
    ),
    path(
        "playlist/get",
        views.GetSongFromPlaylistQueue.as_view(),
        name="get_song_from_playlist_queue",
    ),
    path(
        "playlist/<int:get_id>/mark",
        views.MarkSongAttribute.as_view(),
        name="mark_song_attribute",
    ),
    path(
        "playlist/<int:get_id>/insert",
        views.InsertSongInPlaylistQueue.as_view(),
        name="insert_song_in_playlist_with_order",
    ),
    path(
        "playlist/<int:get_id>/delete",
        views.DeleteSongInPlaylistQueue.as_view(),
        name="delete_song_in_playlist",
    ),
    path(
        "playlist/queue",
        views.ShowSongInPlaylistQueue.as_view(),
        name="show_all_queue",
    ),
]

from django.urls import path
import logging

from player import views

logger = logging.getLogger(__name__)

urlpatterns = [
    path("", views.dashboard, name="player"),
    path(
        "youtube-video-info",
        views.youtube_video_info.as_view(),
        name="youtube-video-info",
    ),
    path(
        "nightbot/order",
        views.order.as_view(),
        name="order",
    ),
    path(
        "playlist/get",
        views.get.as_view(),
        name="get",
    ),
    path(
        "playlist/<int:get_id>/mark",
        views.mark.as_view(),
        name="mark",
    ),
    path(
        "playlist/queue",
        views.order_queue.as_view(),
        name="order_queue",
    ),
]

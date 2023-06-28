from django.urls import re_path
from player import consumers

websocket_urlpatterns = [
    re_path(r"ws/player/$", consumers.PlayerConsumer.as_asgi()),
]

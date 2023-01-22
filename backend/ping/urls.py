from ping import views
from django.urls import path

urlpatterns = [
    path(
        "",
        views.Ping.as_view(),
        name="ping",
    ),
]

from django.urls import path
from userprofile import views

urlpatterns = [
    path("register", views.register, name="register"),
    path("dashboard", views.dashboard, name="dashboard"),
    path("login", views.MyLoginView.as_view(), name="login"),
]

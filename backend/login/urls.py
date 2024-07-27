from django.urls import path
from login import views
import logging

logger = logging.getLogger(__name__)

urlpatterns = [
    path('login', views.Login.as_view(), name='login-page'), # normal login page
    path('register', views.Register.as_view(), name='user-register'),
]
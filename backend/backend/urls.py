from django.contrib import admin
from django.urls import path
from django.urls import include, re_path

from drf_yasg.views import get_schema_view
from drf_yasg import openapi
from drf_yasg.generators import OpenAPISchemaGenerator

from rest_framework.permissions import AllowAny, IsAdminUser

from rest_framework.routers import DefaultRouter

from django.conf import settings

router = DefaultRouter(trailing_slash=False)

URL_PREFIX = 'api'


urlpatterns = []

# pages
urlpatterns += [
    # admin
    path(f"{URL_PREFIX}/__hidden_admin/", admin.site.urls),
]


# app route
urlpatterns += [
    # Router
    path(f"{URL_PREFIX}/", include(router.urls), name="api"),
    # Auth
    path(f"{URL_PREFIX}/auth/", include("custom_jwt.urls")),
    # Google Login
    path(f"{URL_PREFIX}/auth/google/", include("custom_auth.urls")),
    # Ping
    path("player/ping", include("ping.urls"), name="ping"),
    # Player
    path("player/", include("player.urls"), name="player"),
    # Login
    path("player/", include("login.urls"), name="login"),
]


# -------------- START - Swagger View --------------

# Http & Https
class BothHttpAndHttpsSchemaGenerator(OpenAPISchemaGenerator):
    def get_schema(self, request=None, public=False):
        schema = super().get_schema(request, public)
        schema.schemes = ["http", "https"]
        return schema

schema_view = get_schema_view(
    openapi.Info(
        title="Backend service API",
        default_version="v1",
        description="API of backend services.",
    ),
    public=True,
    # permission_classes=(AllowAny,),
    permission_classes = (IsAdminUser,), #is_staff才可使用
    generator_class=BothHttpAndHttpsSchemaGenerator,
)
# --------------- END - Swagger View ----------------


urlpatterns += [
    re_path(
        r"^api/__hidden_swagger(?P<format>\.json|\.yaml)$",
        schema_view.without_ui(cache_timeout=0),
        name="schema-json",
    ),
    re_path(
        r"^api/__hidden_swagger",
        schema_view.with_ui("swagger", cache_timeout=0),
        name="schema-swagger-ui",
    ),
    re_path(
        r"^api/__hidden_redoc",
        schema_view.with_ui("redoc", cache_timeout=0),
        name="schema-redoc",
    ),
]


from django.conf.urls.static import static
urlpatterns += static(settings.STATIC_URL, document_root=settings.STATIC_ROOT)

